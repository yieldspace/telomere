use super::base::WasmBaseParser;
use super::type_checker::TypeChecker;
use super::validate::*;
use super::Result;
use crate::binary::BinaryReader;
use crate::common::BlockReturn;
use crate::common::GlobalType;
use crate::common::LoopParam;
use crate::common::MemArg;
use crate::common::Mut;
use crate::common::RefType;
use crate::common::ResultType;
use crate::parser::core::type_checker::MaybeUnreachable;
use crate::runtime::vm;
use crate::{
    common::{
        BlockType, DataCountVerifier, Elem, FuncIdx, FuncType, Instr, Locals, MemType, Operand,
        TableType, TypeIdx, TypeSection, ValType, ValueSize,
    },
    WasmParserError,
};
use tracing::trace;
fn get_local_addr(ty: &ResultType, locals: &[Locals], idx: u32) -> Result<(ValType, u32)> {
    let mut addr = 0;
    let mut i = 0;
    for t in ty.iter() {
        if idx < i + 1 {
            return Ok((*t, addr));
        }
        addr += t.stack_size().u32();
        i += 1;
    }
    for local in locals {
        if idx < i + local.n {
            addr += (idx - i) * local.t.stack_size().u32();
            return Ok((local.t, addr));
        }
        addr += local.t.stack_size().u32() * local.n;
        i += local.n;
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
    funcidx: FuncIdx,
    mems: &'a [MemType],
    functype: &'a FuncType,
    locals: &'a [Locals],
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
    fn parse_memarg(&mut self, natural_align: u32) -> Result<(usize, MemArg)> {
        let (len, align) = self.parse_u32()?;
        if align > natural_align {
            Err(WasmParserError::InvalidAlignment(align))?;
        }
        let (len2, offset) = self.parse_u32()?;
        Ok((len + len2, MemArg { align, offset }))
    }
    fn parse_inst(
        &mut self,
        data_count_section: &mut DataCountVerifier,
        instrs: &mut Vec<Instr>,
        checker: &mut TypeChecker,
        else_addr: &mut Option<u32>,
        unreachable: &mut bool,
        is_unreachable_if_block: bool,
    ) -> Result<(usize, bool)> {
        let v = self.reader.read_exact_one()?;

        Ok(match v {
            0x00 => {
                trace!("parse_op_unreachable");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_unreachable,
                    });
                    *unreachable = true;
                }
                checker.unreachable();
                (1, false)
            }
            0x01 => (1, false),
            0x02 => {
                let (len, blocktype) = self.parse_block_type()?;
                trace!("parse_op_block: {blocktype:?}");
                let inst_unreachable = *unreachable;
                let mut unreachable = *unreachable;
                if !inst_unreachable {
                    instrs.push(Instr { op: vm::op_block });
                    instrs.push(Instr {
                        operand: Operand {
                            jump_addr: 0xFAFAFAFA,
                        },
                    });
                }

                let index = instrs.len() - 1;
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

                let len2 = self.parse_instrs(
                    data_count_section,
                    instrs,
                    checker,
                    else_addr,
                    &mut unreachable,
                    is_unreachable_if_block,
                )?;
                if !inst_unreachable {
                    instrs[index].operand.jump_addr = instrs.len() as u32;

                    let block_base_stack_size = checker.block_base_stack_size()?;

                    instrs.push(Instr {
                        op: vm::special_block_return,
                    });
                    instrs.push(Instr {
                        operand: Operand {
                            block_return: BlockReturn {
                                stack_top: block_base_stack_size,
                                return_size: blocktype
                                    .return_size(self.types)
                                    .ok_or(WasmParserError::InvalidStackValTypeAny)?,
                            },
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
                let inst_unreachable = *unreachable;
                let mut unreachable = *unreachable;
                if !inst_unreachable {
                    instrs.push(Instr { op: vm::op_loop });
                    instrs.push(Instr {
                        operand: Operand {
                            jump_addr: (instrs.len() - 1) as u32,
                        },
                    });
                }

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
                if !inst_unreachable {
                    let block_base_stack_size = checker.block_base_stack_size()?;
                    instrs.push(Instr {
                        operand: Operand {
                            loop_param: LoopParam {
                                stack_top: block_base_stack_size,
                                param_size: blocktype
                                    .param_size(self.types)
                                    .ok_or(WasmParserError::InvalidStackValTypeAny)?,
                            },
                        },
                    });
                }

                let len2 = self.parse_instrs(
                    data_count_section,
                    instrs,
                    checker,
                    else_addr,
                    &mut unreachable,
                    is_unreachable_if_block,
                )?;
                if !inst_unreachable {
                    let block_base_stack_size = checker.block_base_stack_size()?;

                    instrs.push(Instr {
                        op: vm::special_block_return,
                    });
                    instrs.push(Instr {
                        operand: Operand {
                            block_return: BlockReturn {
                                stack_top: block_base_stack_size,
                                return_size: blocktype
                                    .return_size(self.types)
                                    .ok_or(WasmParserError::InvalidStackValTypeAny)?,
                            },
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
                let mut unreachable = *unreachable;
                let is_unreachable_if_block = unreachable;
                if !is_unreachable_if_block {
                    instrs.push(Instr { op: vm::op_if });
                    instrs.push(Instr {
                        operand: Operand {
                            jump_addr2: (0xFCFCFCFC, 0xFDFDFDFD),
                        },
                    });
                }
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

                let index = instrs.len() - 1;
                let mut else_addr = None;
                let len2 = self.parse_instrs(
                    data_count_section,
                    instrs,
                    checker,
                    &mut else_addr,
                    &mut unreachable,
                    is_unreachable_if_block,
                )?;
                if !is_unreachable_if_block {
                    instrs[index].operand = Operand {
                        jump_addr2: (
                            instrs.len() as u32,
                            else_addr.unwrap_or_else(|| (instrs.len() - 1) as u32),
                        ),
                    };
                }
                match blocktype {
                    BlockType::Void => {
                        if unreachable {
                            checker.reset_stack()?;
                        }
                        checker.leave_block()?;
                    }
                    BlockType::TypeIdx(idx) => {
                        let ty = self
                            .types
                            .get(idx)
                            .ok_or(WasmParserError::InvalidTypeIdx(idx))?;
                        if else_addr.is_none() {
                            if unreachable {
                                checker.reset_stack()?;
                            } else {
                                checker.op(&ty.1 .0, &[])?;
                            }
                            checker.leave_block()?;
                            checker.enter_block(BlockKind::If, blocktype);
                            checker.op(&[], &ty.0 .0)?;
                            unreachable = is_unreachable_if_block;
                        }
                        if unreachable {
                            checker.reset_stack()?;
                        } else {
                            checker.op(&ty.1 .0, &[])?;
                        }
                        checker.leave_block()?;
                        checker.op(&[], &ty.1 .0)?;
                    }
                    BlockType::ValType(ty) => {
                        if else_addr.is_none() {
                            if unreachable {
                                checker.reset_stack()?;
                            } else {
                                checker.op(&[ty], &[])?;
                            }
                            checker.leave_block()?;
                            checker.enter_block(BlockKind::If, blocktype);
                            unreachable = is_unreachable_if_block;
                        }
                        if unreachable {
                            checker.reset_stack()?;
                        } else {
                            checker.op(&[ty], &[])?;
                        }
                        checker.leave_block()?;
                        checker.op(&[], &[ty])?;
                    }
                }

                (1 + len + len2, false)
            }
            0x05 => {
                let inst_unreachable = *unreachable;
                trace!("parse_op_else: {inst_unreachable} {is_unreachable_if_block}");

                *unreachable = is_unreachable_if_block;
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_else });
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
                instrs.push(Instr { op: vm::op_end });

                (1, true)
            }

            0x0C => {
                let (len, idx) = self.parse_u32()?;
                trace!("parse_op_br: {idx}");
                let inst_unreachable = *unreachable;
                *unreachable = true;
                if !inst_unreachable {
                    instrs.push(Instr { op: vm::op_br });
                    instrs.push(Instr {
                        operand: Operand { u32: idx },
                    });
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
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_br_if });
                    instrs.push(Instr {
                        operand: Operand { u32: idx },
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
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_br_table,
                    });
                    instrs.push(Instr {
                        operand: Operand {
                            u32: idxs.len() as u32,
                        },
                    });
                    for idx in &idxs {
                        instrs.push(Instr {
                            operand: Operand { u32: *idx },
                        });
                    }
                    instrs.push(Instr {
                        operand: Operand { u32: default_idx },
                    });
                }
                checker.unreachable();
                *unreachable = true;
                (1 + len + len2, false)
            }
            0x0F => {
                trace!("parse_op_return");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_return });
                }
                checker.op(&self.functype.1 .0, &[])?;
                checker.unreachable();

                *unreachable = true;
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
                checker.op_func_type(ty)?;

                if !*unreachable {
                    instrs.push(Instr { op: vm::op_call });
                    instrs.push(Instr {
                        operand: Operand { u32: idx },
                    });
                }
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
                checker.op(&[ValType::I32], &[])?;
                let ty = self
                    .types
                    .get(TypeIdx(typeidx))
                    .ok_or(WasmParserError::InvalidTypeIdx(TypeIdx(typeidx)))?;
                checker.op_func_type(ty)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_call_indirect,
                    });
                    instrs.push(Instr {
                        operand: Operand { u32: tableidx },
                    });
                    instrs.push(Instr {
                        operand: Operand { u32: typeidx },
                    });
                }

                (1 + len + len2, false)
            }
            0x1A => {
                trace!("parse_op_drop");
                let x = checker.pop()?;

                if !*unreachable {
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
                    if !*unreachable {
                        instrs.push(Instr { op: vm::op_select });
                        instrs.push(Instr {
                            operand: Operand {
                                select: x.stack_size().u32(),
                            },
                        });
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
                        if !*unreachable {
                            instrs.push(Instr { op: vm::op_select });
                            instrs.push(Instr {
                                operand: Operand {
                                    select: x.stack_size().u32(),
                                },
                            });
                        }
                        checker.push(x);
                    } else {
                        assert!(*unreachable);
                        checker.push_any();
                    }
                }

                (1, false)
            }
            0x1C => {
                let (len, operand) = self.parse_vec(Self::parse_valtype)?;
                trace!("parse_op_select_with_param: {operand:?}");
                if !*unreachable {
                    if operand.len() != 1 {
                        Err(WasmParserError::InvalidResultArity)?;
                    }
                    checker.op(&[ValType::I32], &[])?;
                    checker.op(&operand, &[])?;
                    checker.op(&operand, &operand)?;
                    let bytes = operand.iter().map(|v| v.stack_size().u32()).sum();
                    instrs.push(Instr { op: vm::op_select });
                    instrs.push(Instr {
                        operand: Operand { select: bytes },
                    });
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

                if !*unreachable {
                    match ty.stack_size() {
                        ValueSize::Byte4 => instrs.push(Instr {
                            op: vm::op_local_get4,
                        }),
                        ValueSize::Byte8 => instrs.push(Instr {
                            op: vm::op_local_get8,
                        }),
                        ValueSize::Byte16 => todo!(),
                    }
                    instrs.push(Instr {
                        operand: Operand { local_addr: addr },
                    });
                }
                checker.op(&[], &[ty])?;

                (1 + len, false)
            }
            0x21 => {
                trace!("parse_op_local_set");
                let (len, idx) = self.parse_u32()?;
                let (ty, addr) = get_local_addr(&self.functype.0, self.locals, idx)?;

                if !*unreachable {
                    match ty.stack_size() {
                        ValueSize::Byte4 => instrs.push(Instr {
                            op: vm::op_local_set4,
                        }),
                        ValueSize::Byte8 => instrs.push(Instr {
                            op: vm::op_local_set8,
                        }),
                        ValueSize::Byte16 => todo!(),
                    }
                    instrs.push(Instr {
                        operand: Operand { local_addr: addr },
                    });
                }
                checker.op(&[ty], &[])?;

                (1 + len, false)
            }
            0x22 => {
                trace!("parse_op_local_tee");
                let (len, idx) = self.parse_u32()?;
                let (ty, addr) = get_local_addr(&self.functype.0, self.locals, idx)?;

                if !*unreachable {
                    match ty.stack_size() {
                        ValueSize::Byte4 => instrs.push(Instr {
                            op: vm::op_local_tee4,
                        }),
                        ValueSize::Byte8 => instrs.push(Instr {
                            op: vm::op_local_tee8,
                        }),
                        ValueSize::Byte16 => todo!(),
                    }
                    instrs.push(Instr {
                        operand: Operand { local_addr: addr },
                    });
                }
                checker.op(&[ty], &[ty])?;

                (1 + len, false)
            }
            0x23 => {
                trace!("parse_op_global_get");

                let (len, idx) = self.parse_u32()?;
                let ty = self
                    .globals
                    .get(idx as usize)
                    .ok_or(WasmParserError::InvalidGlobalAccess)?;
                if !*unreachable {
                    match ty.0.stack_size() {
                        ValueSize::Byte4 => instrs.push(Instr {
                            op: vm::op_global_get4,
                        }),
                        ValueSize::Byte8 => instrs.push(Instr {
                            op: vm::op_global_get8,
                        }),
                        ValueSize::Byte16 => todo!(),
                    }
                    checker.op(&[], &[ty.0])?;

                    instrs.push(Instr {
                        operand: Operand { u32: idx },
                    });
                }
                (1 + len, false)
            }
            0x24 => {
                trace!("parse_op_global_set");
                let (len, idx) = self.parse_u32()?;
                if !*unreachable {
                    let ty = self
                        .globals
                        .get(idx as usize)
                        .ok_or(WasmParserError::InvalidGlobalAccess)?;
                    if ty.1 != Mut::Var {
                        Err(WasmParserError::InvalidGlobalAccess)?
                    }
                    match ty.0.stack_size() {
                        ValueSize::Byte4 => instrs.push(Instr {
                            op: vm::op_global_set4,
                        }),
                        ValueSize::Byte8 => instrs.push(Instr {
                            op: vm::op_global_set8,
                        }),
                        ValueSize::Byte16 => todo!(),
                    }
                    checker.op(&[ty.0], &[])?;

                    instrs.push(Instr {
                        operand: Operand { u32: idx },
                    });
                }
                (1 + len, false)
            }
            0x25 => {
                trace!("parse_op_table_get");
                let (len, idx) = self.parse_u32()?;
                if !*unreachable {
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
                }
                (1 + len, false)
            }
            0x26 => {
                trace!("parse_op_table_set");
                let (len, idx) = self.parse_u32()?;
                if !*unreachable {
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
                }
                (1 + len, false)
            }
            0x28 => {
                trace!("parse_op_i32_load");
                assert_memory(self.mems)?;
                let (len, memarg) = self.parse_memarg(4)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_load,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    checker.load_op(ValType::I32)?;
                }
                (1 + len, false)
            }
            0x29 => {
                trace!("parse_op_i64_load");
                assert_memory(self.mems)?;
                let (len, memarg) = self.parse_memarg(8)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_load,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    checker.load_op(ValType::I64)?;
                }
                (1 + len, false)
            }
            0x2A => {
                trace!("parse_op_f32_load");
                assert_memory(self.mems)?;
                let (len, memarg) = self.parse_memarg(4)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f32_load,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    checker.load_op(ValType::F32)?;
                }
                (1 + len, false)
            }
            0x2B => {
                trace!("parse_op_f64_load");
                assert_memory(self.mems)?;
                let (len, memarg) = self.parse_memarg(8)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f64_load,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    checker.load_op(ValType::F64)?;
                }
                (1 + len, false)
            }
            0x2C => {
                trace!("parse_op_i32_load8_s");
                assert_memory(self.mems)?;
                let (len, memarg) = self.parse_memarg(1)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_load8_s,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    checker.load_op(ValType::I32)?;
                }
                (1 + len, false)
            }
            0x2D => {
                trace!("parse_op_i32_load8_u");
                assert_memory(self.mems)?;
                let (len, memarg) = self.parse_memarg(1)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_load8_u,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    checker.load_op(ValType::I32)?;
                }
                (1 + len, false)
            }
            0x2E => {
                trace!("parse_op_i32_load16_s");
                assert_memory(self.mems)?;
                let (len, memarg) = self.parse_memarg(2)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_load16_s,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    checker.load_op(ValType::I32)?;
                }
                (1 + len, false)
            }
            0x2F => {
                trace!("parse_op_i32_load16_u");
                assert_memory(self.mems)?;
                let (len, memarg) = self.parse_memarg(2)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_load16_u,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    checker.load_op(ValType::I32)?;
                }
                (1 + len, false)
            }
            0x30 => {
                trace!("parse_op_i64_load8_s");
                assert_memory(self.mems)?;
                let (len, memarg) = self.parse_memarg(1)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_load8_s,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    checker.load_op(ValType::I64)?;
                }
                (1 + len, false)
            }
            0x31 => {
                trace!("parse_op_i64_load8_u");
                assert_memory(self.mems)?;
                let (len, memarg) = self.parse_memarg(1)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_load8_u,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    checker.load_op(ValType::I64)?;
                }
                (1 + len, false)
            }
            0x32 => {
                trace!("parse_op_i64_load16_s");
                assert_memory(self.mems)?;
                let (len, memarg) = self.parse_memarg(2)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_load16_s,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    checker.load_op(ValType::I64)?;
                }
                (1 + len, false)
            }
            0x33 => {
                trace!("parse_op_i64_load16_u");
                assert_memory(self.mems)?;
                let (len, memarg) = self.parse_memarg(2)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_load16_u,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    checker.load_op(ValType::I64)?;
                }
                (1 + len, false)
            }
            0x34 => {
                trace!("parse_op_i64_load32_s");
                assert_memory(self.mems)?;
                let (len, memarg) = self.parse_memarg(4)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_load32_s,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    checker.load_op(ValType::I64)?;
                }
                (1 + len, false)
            }
            0x35 => {
                trace!("parse_op_i64_load32_u");
                assert_memory(self.mems)?;
                let (len, memarg) = self.parse_memarg(4)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_load32_u,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    checker.load_op(ValType::I64)?;
                }
                (1 + len, false)
            }
            0x36 => {
                trace!("parse_op_i32_store");
                assert_memory(self.mems)?;
                let (len, memarg) = self.parse_memarg(4)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_store,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    checker.store_op(ValType::I32)?;
                }
                (1 + len, false)
            }
            0x37 => {
                trace!("parse_op_i64_store");
                assert_memory(self.mems)?;
                let (len, memarg) = self.parse_memarg(8)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_store,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                }
                checker.store_op(ValType::I64)?;

                (1 + len, false)
            }
            0x38 => {
                trace!("parse_op_f32_store");
                assert_memory(self.mems)?;
                let (len, memarg) = self.parse_memarg(4)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f32_store,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    checker.store_op(ValType::F32)?;
                }
                (1 + len, false)
            }
            0x39 => {
                trace!("parse_op_f64_store");
                assert_memory(self.mems)?;
                let (len, memarg) = self.parse_memarg(8)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f64_store,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                }
                checker.store_op(ValType::F64)?;

                (1 + len, false)
            }
            0x3A => {
                trace!("parse_op_i32_store8");
                assert_memory(self.mems)?;
                let (len, memarg) = self.parse_memarg(1)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_store8,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                }
                checker.store_op(ValType::I32)?;

                (1 + len, false)
            }
            0x3B => {
                trace!("parse_op_i32_store16");
                assert_memory(self.mems)?;
                let (len, memarg) = self.parse_memarg(2)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_store16,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    checker.store_op(ValType::I32)?;
                }
                (1 + len, false)
            }
            0x3C => {
                trace!("parse_op_i64_store8");
                assert_memory(self.mems)?;
                let (len, memarg) = self.parse_memarg(1)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_store8,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    checker.store_op(ValType::I64)?;
                }
                (1 + len, false)
            }
            0x3D => {
                trace!("parse_op_i64_store16");
                assert_memory(self.mems)?;
                let (len, memarg) = self.parse_memarg(2)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_store16,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                }
                checker.store_op(ValType::I64)?;

                (1 + len, false)
            }
            0x3E => {
                trace!("parse_op_i64_store32");
                assert_memory(self.mems)?;
                let (len, memarg) = self.parse_memarg(4)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_store32,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    checker.store_op(ValType::I64)?;
                }
                (1 + len, false)
            }
            0x3F => {
                trace!("parse_op_mem_size");
                let next = self.reader.read_exact_one()?;
                assert_memory(self.mems)?;
                if next != 0x00 {
                    Err(WasmParserError::InvalidInstruction([0x3F, next, 0, 0]))?
                }
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_mem_size,
                    });
                    checker.op(&[], &[ValType::I32])?;
                }
                (2, false)
            }
            0x40 => {
                trace!("parse_op_mem_grow");
                let next = self.reader.read_exact_one()?;
                if next != 0x00 {
                    Err(WasmParserError::InvalidInstruction([0x40, next, 0, 0]))?
                }
                assert_memory(self.mems)?;
                if !*unreachable {
                    checker.op(&[ValType::I32], &[ValType::I32])?;
                    instrs.push(Instr {
                        op: vm::op_mem_grow,
                    });
                }
                (2, false)
            }
            0x41 => {
                trace!("parse_op_i32_const");
                let (len, operand) = self.parse_i32()?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_const,
                    });
                    instrs.push(Instr {
                        operand: Operand { i32: operand },
                    });
                }
                checker.op(&[], &[ValType::I32])?;
                (1 + len, false)
            }
            0x42 => {
                trace!("parse_op_i64_const");
                let (len, operand) = self.parse_i64()?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_const,
                    });
                    instrs.push(Instr {
                        operand: Operand { i64: operand },
                    });
                }
                checker.op(&[], &[ValType::I64])?;
                (1 + len, false)
            }
            0x43 => {
                trace!("parse_op_f32_const");
                let (len, operand) = self.parse_f32()?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f32_const,
                    });
                    instrs.push(Instr {
                        operand: Operand { f32: operand },
                    });
                }
                checker.op(&[], &[ValType::F32])?;

                (1 + len, false)
            }
            0x44 => {
                trace!("parse_op_f64_const");
                let (len, operand) = self.parse_f64()?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f64_const,
                    });
                    instrs.push(Instr {
                        operand: Operand { f64: operand },
                    });
                }
                checker.op(&[], &[ValType::F64])?;

                (1 + len, false)
            }
            0x45 => {
                trace!("parse_op_i32_eqz");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i32_eqz });
                }
                checker.op(&[ValType::I32], &[ValType::I32])?;
                (1, false)
            }
            0x46 => {
                trace!("parse_op_i32_eq");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i32_eq });
                }
                checker.cond_op(ValType::I32)?;
                (1, false)
            }
            0x47 => {
                trace!("parse_op_i32_ne");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i32_ne });
                }
                checker.cond_op(ValType::I32)?;
                (1, false)
            }
            0x48 => {
                trace!("parse_op_i32_lt_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_lt_s,
                    });
                }
                checker.cond_op(ValType::I32)?;
                (1, false)
            }
            0x49 => {
                trace!("parse_op_i32_lt_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_lt_u,
                    });
                }
                checker.cond_op(ValType::I32)?;
                (1, false)
            }
            0x4A => {
                trace!("parse_op_i32_gt_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_gt_s,
                    });
                }
                checker.cond_op(ValType::I32)?;
                (1, false)
            }
            0x4B => {
                trace!("parse_op_i32_gt_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_gt_u,
                    });
                }
                checker.cond_op(ValType::I32)?;
                (1, false)
            }
            0x4C => {
                trace!("parse_op_i32_le_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_le_s,
                    });
                }
                checker.cond_op(ValType::I32)?;
                (1, false)
            }
            0x4D => {
                trace!("parse_op_i32_le_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_le_u,
                    });
                }
                checker.cond_op(ValType::I32)?;
                (1, false)
            }
            0x4E => {
                trace!("parse_op_i32_ge_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_ge_s,
                    });
                }
                checker.cond_op(ValType::I32)?;
                (1, false)
            }
            0x4F => {
                trace!("parse_op_i32_ge_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_ge_u,
                    });
                }
                checker.cond_op(ValType::I32)?;
                (1, false)
            }
            0x50 => {
                trace!("parse_op_i64_eqz");
                checker.op(&[ValType::I64], &[ValType::I32])?;

                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i64_eqz });
                }

                (1, false)
            }
            0x51 => {
                trace!("parse_op_i64_eq");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i64_eq });
                }
                checker.cond_op(ValType::I64)?;
                (1, false)
            }
            0x52 => {
                trace!("parse_op_i64_ne");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i64_ne });
                }
                checker.cond_op(ValType::I64)?;
                (1, false)
            }
            0x53 => {
                trace!("parse_op_i64_lt_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_lt_s,
                    });
                }
                checker.cond_op(ValType::I64)?;
                (1, false)
            }
            0x54 => {
                trace!("parse_op_i64_lt_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_lt_u,
                    });
                }
                checker.cond_op(ValType::I64)?;
                (1, false)
            }
            0x55 => {
                trace!("parse_op_i64_gt_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_gt_s,
                    });
                }
                checker.cond_op(ValType::I64)?;
                (1, false)
            }
            0x56 => {
                trace!("parse_op_i64_gt_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_gt_u,
                    });
                }
                checker.cond_op(ValType::I64)?;
                (1, false)
            }
            0x57 => {
                trace!("parse_op_i64_le_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_le_s,
                    });
                }
                checker.cond_op(ValType::I64)?;
                (1, false)
            }
            0x58 => {
                trace!("parse_op_i64_le_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_le_u,
                    });
                }
                checker.cond_op(ValType::I64)?;
                (1, false)
            }
            0x59 => {
                trace!("parse_op_i64_ge_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_ge_s,
                    });
                }
                checker.cond_op(ValType::I64)?;
                (1, false)
            }
            0x5A => {
                trace!("parse_op_i64_ge_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_ge_u,
                    });
                }
                checker.cond_op(ValType::I64)?;
                (1, false)
            }
            0x5B => {
                trace!("parse_op_f32_eq");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f32_eq });
                }
                checker.cond_op(ValType::F32)?;
                (1, false)
            }
            0x5C => {
                trace!("parse_op_f32_ne");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f32_ne });
                }
                checker.cond_op(ValType::F32)?;
                (1, false)
            }
            0x5D => {
                trace!("parse_op_f32_lt");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f32_lt });
                }
                checker.cond_op(ValType::F32)?;
                (1, false)
            }
            0x5E => {
                trace!("parse_op_f32_gt");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f32_gt });
                }
                checker.cond_op(ValType::F32)?;
                (1, false)
            }
            0x5F => {
                trace!("parse_op_f32_le");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f32_le });
                }
                checker.cond_op(ValType::F32)?;
                (1, false)
            }
            0x60 => {
                trace!("parse_op_f32_ge");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f32_ge });
                }
                checker.cond_op(ValType::F32)?;
                (1, false)
            }
            0x61 => {
                trace!("parse_op_f64_eq");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f64_eq });
                }
                checker.cond_op(ValType::F64)?;
                (1, false)
            }
            0x62 => {
                trace!("parse_op_f64_ne");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f64_ne });
                }
                checker.cond_op(ValType::F64)?;
                (1, false)
            }
            0x63 => {
                trace!("parse_op_f64_lt");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f64_lt });
                }
                checker.cond_op(ValType::F64)?;
                (1, false)
            }
            0x64 => {
                trace!("parse_op_f64_gt");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f64_gt });
                }
                checker.cond_op(ValType::F64)?;
                (1, false)
            }
            0x65 => {
                trace!("parse_op_f64_le");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f64_le });
                }
                checker.cond_op(ValType::F64)?;
                (1, false)
            }
            0x66 => {
                trace!("parse_op_f64_ge");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f64_ge });
                }
                checker.cond_op(ValType::F64)?;
                (1, false)
            }
            0x67 => {
                trace!("parse_op_i32_clz");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i32_clz });
                }
                checker.op(&[ValType::I32], &[ValType::I32])?;

                (1, false)
            }
            0x68 => {
                trace!("parse_op_i32_ctz");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i32_ctz });
                }
                checker.op(&[ValType::I32], &[ValType::I32])?;

                (1, false)
            }
            0x69 => {
                trace!("parse_op_i32_popcnt");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_popcnt,
                    });
                    checker.op(&[ValType::I32], &[ValType::I32])?;
                }
                (1, false)
            }
            0x6A => {
                trace!("parse_op_i32_add");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i32_add });
                    checker.unary_op(ValType::I32)?;
                }
                (1, false)
            }
            0x6B => {
                trace!("parse_op_i32_sub");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i32_sub });
                }
                checker.unary_op(ValType::I32)?;
                (1, false)
            }
            0x6C => {
                trace!("parse_op_i32_mul");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i32_mul });
                }
                checker.unary_op(ValType::I32)?;

                (1, false)
            }
            0x6D => {
                trace!("parse_op_i32_div_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_div_s,
                    });
                    checker.unary_op(ValType::I32)?;
                }
                (1, false)
            }
            0x6E => {
                trace!("parse_op_i32_div_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_div_u,
                    });
                    checker.unary_op(ValType::I32)?;
                }
                (1, false)
            }
            0x6F => {
                trace!("parse_op_i32_rem_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_rem_s,
                    });
                    checker.unary_op(ValType::I32)?;
                }
                (1, false)
            }
            0x70 => {
                trace!("parse_op_i32_rem_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_rem_u,
                    });
                    checker.unary_op(ValType::I32)?;
                }
                (1, false)
            }
            0x71 => {
                trace!("parse_op_i32_and");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i32_and });
                    checker.unary_op(ValType::I32)?;
                }
                (1, false)
            }
            0x72 => {
                trace!("parse_op_i32_or");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i32_or });
                    checker.unary_op(ValType::I32)?;
                }
                (1, false)
            }
            0x73 => {
                trace!("parse_op_i32_xor");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i32_xor });
                    checker.unary_op(ValType::I32)?;
                }
                (1, false)
            }
            0x74 => {
                trace!("parse_op_i32_shl");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i32_shl });
                    checker.unary_op(ValType::I32)?;
                }
                (1, false)
            }
            0x75 => {
                trace!("parse_op_i32_shr_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_shr_s,
                    });
                    checker.unary_op(ValType::I32)?;
                }
                (1, false)
            }
            0x76 => {
                trace!("parse_op_i32_shr_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_shr_u,
                    });
                    checker.unary_op(ValType::I32)?;
                }
                (1, false)
            }
            0x77 => {
                trace!("parse_op_i32_rotl");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_rotl,
                    });
                    checker.unary_op(ValType::I32)?;
                }
                (1, false)
            }
            0x78 => {
                trace!("parse_op_i32_rotr");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_rotr,
                    });
                    checker.unary_op(ValType::I32)?;
                }
                (1, false)
            }
            0x79 => {
                trace!("parse_op_i64_clz");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i64_clz });
                    checker.binary_op(ValType::I64)?;
                }
                (1, false)
            }
            0x7A => {
                trace!("parse_op_i64_ctz");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i64_ctz });
                    checker.binary_op(ValType::I64)?;
                }
                (1, false)
            }
            0x7B => {
                trace!("parse_op_i64_popcnt");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_popcnt,
                    });
                    checker.binary_op(ValType::I64)?;
                }
                (1, false)
            }
            0x7C => {
                trace!("parse_op_i64_add");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i64_add });
                    checker.unary_op(ValType::I64)?;
                }
                (1, false)
            }
            0x7D => {
                trace!("parse_op_i64_sub");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i64_sub });
                }
                checker.unary_op(ValType::I64)?;

                (1, false)
            }
            0x7E => {
                trace!("parse_op_i64_mul");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i64_mul });
                    checker.unary_op(ValType::I64)?;
                }
                (1, false)
            }
            0x7F => {
                trace!("parse_op_i64_div_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_div_s,
                    });
                    checker.unary_op(ValType::I64)?;
                }
                (1, false)
            }
            0x80 => {
                trace!("parse_op_i64_div_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_div_u,
                    });
                    checker.unary_op(ValType::I64)?;
                }
                (1, false)
            }
            0x81 => {
                trace!("parse_op_i64_rem_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_rem_s,
                    });
                    checker.unary_op(ValType::I64)?;
                }
                (1, false)
            }
            0x82 => {
                trace!("parse_op_i64_rem_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_rem_u,
                    });
                    checker.unary_op(ValType::I64)?;
                }
                (1, false)
            }
            0x83 => {
                trace!("parse_op_i64_and");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i64_and });
                    checker.unary_op(ValType::I64)?;
                }
                (1, false)
            }
            0x84 => {
                trace!("parse_op_i64_or");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i64_or });
                    checker.unary_op(ValType::I64)?;
                }
                (1, false)
            }
            0x85 => {
                trace!("parse_op_i64_xor");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i64_xor });
                    checker.unary_op(ValType::I64)?;
                }
                (1, false)
            }
            0x86 => {
                trace!("parse_op64_shl");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i64_shl });
                    checker.unary_op(ValType::I64)?;
                }
                (1, false)
            }
            0x87 => {
                trace!("parse_op_i64_shr_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_shr_s,
                    });
                    checker.unary_op(ValType::I64)?;
                }
                (1, false)
            }
            0x88 => {
                trace!("parse_op_i64_shr_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_shr_u,
                    });
                    checker.unary_op(ValType::I64)?;
                }
                (1, false)
            }
            0x89 => {
                trace!("parse_op_i64_rotl");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_rotl,
                    });
                    checker.unary_op(ValType::I64)?;
                }
                (1, false)
            }
            0x8A => {
                trace!("parse_op_i64_rotr");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_rotr,
                    });
                    checker.unary_op(ValType::I64)?;
                }
                (1, false)
            }
            0x8B => {
                trace!("parse_op_f32_abs");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f32_abs });
                    checker.binary_op(ValType::F32)?;
                }
                (1, false)
            }
            0x8C => {
                trace!("parse_op_f32_neg");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f32_neg });
                }
                checker.binary_op(ValType::F32)?;

                (1, false)
            }
            0x8D => {
                trace!("parse_op_f32_ceil");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f32_ceil,
                    });
                    checker.binary_op(ValType::F32)?;
                }
                (1, false)
            }
            0x8E => {
                trace!("parse_op_f32_floor");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f32_floor,
                    });
                    checker.binary_op(ValType::F32)?;
                }
                (1, false)
            }
            0x8F => {
                trace!("parse_op_f32_trunc");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f32_trunc,
                    });
                    checker.binary_op(ValType::F32)?;
                }
                (1, false)
            }
            0x90 => {
                trace!("parse_op_f32_nearest");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f32_nearest,
                    });
                    checker.binary_op(ValType::F32)?;
                }
                (1, false)
            }
            0x91 => {
                trace!("parse_op_f32_sqrt");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f32_sqrt,
                    });
                    checker.binary_op(ValType::F32)?;
                }
                (1, false)
            }
            0x92 => {
                trace!("parse_op_f32_add");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f32_add });
                    checker.unary_op(ValType::F32)?;
                }
                (1, false)
            }
            0x93 => {
                trace!("parse_op_f32_sub");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f32_sub });
                    checker.unary_op(ValType::F32)?;
                }
                (1, false)
            }
            0x94 => {
                trace!("parse_op_f32_mul");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f32_mul });
                    checker.unary_op(ValType::F32)?;
                }
                (1, false)
            }
            0x95 => {
                trace!("parse_op_f32_div");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f32_div });
                    checker.unary_op(ValType::F32)?;
                }
                (1, false)
            }
            0x96 => {
                trace!("parse_op_f32_min");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f32_min });
                    checker.unary_op(ValType::F32)?;
                }
                (1, false)
            }
            0x97 => {
                trace!("parse_op_f32_max");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f32_max });
                    checker.unary_op(ValType::F32)?;
                }
                (1, false)
            }
            0x98 => {
                trace!("parse_op_f32_copysign");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f32_copysign,
                    });
                    checker.unary_op(ValType::F32)?;
                }
                (1, false)
            }
            0x99 => {
                trace!("parse_op_f64_abs");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f64_abs });
                    checker.binary_op(ValType::F64)?;
                }
                (1, false)
            }
            0x9A => {
                trace!("parse_op_f64_neg");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f64_neg });
                    checker.binary_op(ValType::F64)?;
                }
                (1, false)
            }
            0x9B => {
                trace!("parse_op_f64_ceil");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f64_ceil,
                    });
                    checker.binary_op(ValType::F64)?;
                }
                (1, false)
            }
            0x9C => {
                trace!("parse_op_f64_floor");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f64_floor,
                    });
                    checker.binary_op(ValType::F64)?;
                }
                (1, false)
            }
            0x9D => {
                trace!("parse_op_f64_trunc");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f64_trunc,
                    });
                    checker.binary_op(ValType::F64)?;
                }
                (1, false)
            }
            0x9E => {
                trace!("parse_op_f64_nearest");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f64_nearest,
                    });
                    checker.binary_op(ValType::F64)?;
                }
                (1, false)
            }

            0x9F => {
                trace!("parse_op_f64_sqrt");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f64_sqrt,
                    });
                    checker.binary_op(ValType::F64)?;
                }
                (1, false)
            }
            0xA0 => {
                trace!("parse_op_f64_add");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f64_add });
                    checker.unary_op(ValType::F64)?;
                }
                (1, false)
            }
            0xA1 => {
                trace!("parse_op_f64_sub");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f64_sub });
                    checker.unary_op(ValType::F64)?;
                }
                (1, false)
            }
            0xA2 => {
                trace!("parse_op_f64_mul");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f64_mul });
                    checker.unary_op(ValType::F64)?;
                }
                (1, false)
            }
            0xA3 => {
                trace!("parse_op_f64_div");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f64_div });
                    checker.unary_op(ValType::F64)?;
                }
                (1, false)
            }
            0xA4 => {
                trace!("parse_op_f64_min");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f64_min });
                    checker.unary_op(ValType::F64)?;
                }
                (1, false)
            }
            0xA5 => {
                trace!("parse_op_f64_max");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f64_max });
                    checker.unary_op(ValType::F64)?;
                }
                (1, false)
            }
            0xA6 => {
                trace!("parse_op_f64_copysign");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f64_copysign,
                    });
                    checker.unary_op(ValType::F64)?;
                }
                (1, false)
            }
            0xA7 => {
                trace!("parse_op_i32_wrap_i64");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_wrap_i64,
                    });
                    checker.op(&[ValType::I64], &[ValType::I32])?;
                }
                (1, false)
            }
            0xA8 => {
                trace!("parse_op_i32_trunc_f32_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_trunc_f32_s,
                    });
                    checker.op(&[ValType::F32], &[ValType::I32])?;
                }
                (1, false)
            }
            0xA9 => {
                trace!("parse_op_i32_trunc_f32_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_trunc_f32_u,
                    });
                    checker.op(&[ValType::F32], &[ValType::I32])?;
                }
                (1, false)
            }
            0xAA => {
                trace!("parse_op_i32_trunc_f64_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_trunc_f64_s,
                    });
                    checker.op(&[ValType::F64], &[ValType::I32])?;
                }
                (1, false)
            }
            0xAB => {
                trace!("parse_op_i32_trunc_f64_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_trunc_f64_u,
                    });
                    checker.op(&[ValType::F64], &[ValType::I32])?;
                }
                (1, false)
            }
            0xAC => {
                trace!("parse_op_i64_extend_i32_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_extend_i32_s,
                    });
                    checker.op(&[ValType::I32], &[ValType::I64])?;
                }
                (1, false)
            }
            0xAD => {
                trace!("parse_op_i64_extend_i32_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_extend_i32_u,
                    });
                }
                checker.op(&[ValType::I32], &[ValType::I64])?;

                (1, false)
            }
            0xAE => {
                trace!("parse_op_i64_trunc_f32_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_trunc_f32_s,
                    });
                    checker.op(&[ValType::F32], &[ValType::I64])?;
                }
                (1, false)
            }
            0xAF => {
                trace!("parse_op_i64_trunc_f32_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_trunc_f32_u,
                    });
                    checker.op(&[ValType::F32], &[ValType::I64])?;
                }
                (1, false)
            }
            0xB0 => {
                trace!("parse_op_i64_trunc_f64_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_trunc_f64_s,
                    });
                    checker.op(&[ValType::F64], &[ValType::I64])?;
                }
                (1, false)
            }
            0xB1 => {
                trace!("parse_op_i64_trunc_f64_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_trunc_f64_u,
                    });
                    checker.op(&[ValType::F64], &[ValType::I64])?;
                }
                (1, false)
            }
            0xB2 => {
                trace!("parse_op_f32_convert_i32_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f32_convert_i32_s,
                    });
                    checker.op(&[ValType::I32], &[ValType::F32])?;
                }
                (1, false)
            }
            0xB3 => {
                trace!("parse_op_f32_convert_i32_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f32_convert_i32_u,
                    });
                    checker.op(&[ValType::I32], &[ValType::F32])?;
                }
                (1, false)
            }
            0xB4 => {
                trace!("parse_op_f32_convert_i64_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f32_convert_i64_s,
                    });
                    checker.op(&[ValType::I64], &[ValType::F32])?;
                }
                (1, false)
            }
            0xB5 => {
                trace!("parse_op_f32_convert_i64_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f32_convert_i64_u,
                    });
                    checker.op(&[ValType::I64], &[ValType::F32])?;
                }
                (1, false)
            }
            0xB6 => {
                trace!("parse_op_f32_demote_f64");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f32_demote_f64,
                    });
                    checker.op(&[ValType::F64], &[ValType::F32])?;
                }
                (1, false)
            }
            0xB7 => {
                trace!("parse_op_f64_convert_i32_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f64_convert_i32_s,
                    });
                    checker.op(&[ValType::I32], &[ValType::F64])?;
                }
                (1, false)
            }
            0xB8 => {
                trace!("parse_op_f64_convert_i32_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f64_convert_i32_u,
                    });
                    checker.op(&[ValType::I32], &[ValType::F64])?;
                }
                (1, false)
            }
            0xB9 => {
                trace!("parse_op_f64_convert_i64_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f64_convert_i64_s,
                    });
                    checker.op(&[ValType::I64], &[ValType::F64])?;
                }
                (1, false)
            }
            0xBA => {
                trace!("parse_op_f64_convert_i64_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f64_convert_i64_u,
                    });
                    checker.op(&[ValType::I64], &[ValType::F64])?;
                }
                (1, false)
            }
            0xBB => {
                trace!("parse_op_f64_promote_f32");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f64_promote_f32,
                    });
                    checker.op(&[ValType::F32], &[ValType::F64])?;
                }
                (1, false)
            }
            0xBC => {
                trace!("parse_op_i32_reinterpret_f32");
                if !*unreachable {
                    checker.op(&[ValType::F32], &[ValType::I32])?;
                }
                (1, false)
            }
            0xBD => {
                trace!("parse_op_i64_reinterpret_f64");
                if !*unreachable {
                    checker.op(&[ValType::F64], &[ValType::I64])?;
                }
                (1, false)
            }
            0xBE => {
                trace!("parse_op_f32_reinterpret_i32");
                if !*unreachable {
                    checker.op(&[ValType::I32], &[ValType::F32])?;
                }
                (1, false)
            }
            0xBF => {
                trace!("parse_op_f64_reinterpret_i64");
                if !*unreachable {
                    checker.op(&[ValType::I64], &[ValType::F64])?;
                }
                (1, false)
            }
            0xFC => {
                let (len, next) = self.parse_u32()?;
                match next {
                    0 => {
                        trace!("parse_op_i32_trunc_sat_f32_s");
                        if !*unreachable {
                            instrs.push(Instr {
                                op: vm::op_i32_trunc_sat_f32_s,
                            });
                            checker.op(&[ValType::F32], &[ValType::I32])?;
                        }
                        (1 + len, false)
                    }
                    1 => {
                        trace!("parse_op_i32_trunc_sat_f32_u");
                        if !*unreachable {
                            instrs.push(Instr {
                                op: vm::op_i32_trunc_sat_f32_u,
                            });
                            checker.op(&[ValType::F32], &[ValType::I32])?;
                        }
                        (1 + len, false)
                    }
                    2 => {
                        trace!("parse_op_i32_trunc_sat_f64_s");
                        if !*unreachable {
                            instrs.push(Instr {
                                op: vm::op_i32_trunc_sat_f64_s,
                            });
                            checker.op(&[ValType::F64], &[ValType::I32])?;
                        }
                        (1 + len, false)
                    }
                    3 => {
                        trace!("parse_op_i32_trunc_sat_f64_u");
                        if !*unreachable {
                            instrs.push(Instr {
                                op: vm::op_i32_trunc_sat_f64_u,
                            });
                            checker.op(&[ValType::F64], &[ValType::I32])?;
                        }
                        (1 + len, false)
                    }
                    4 => {
                        trace!("parse_op_i64_trunc_sat_f32_s");
                        if !*unreachable {
                            instrs.push(Instr {
                                op: vm::op_i64_trunc_sat_f32_s,
                            });
                            checker.op(&[ValType::F32], &[ValType::I64])?;
                        }
                        (1 + len, false)
                    }
                    5 => {
                        trace!("parse_op_i64_trunc_sat_f32_u");
                        if !*unreachable {
                            instrs.push(Instr {
                                op: vm::op_i64_trunc_sat_f32_u,
                            });
                            checker.op(&[ValType::F32], &[ValType::I64])?;
                        }
                        (1 + len, false)
                    }
                    6 => {
                        trace!("parse_op_i64_trunc_sat_f64_s");
                        if !*unreachable {
                            instrs.push(Instr {
                                op: vm::op_i64_trunc_sat_f64_s,
                            });
                            checker.op(&[ValType::F64], &[ValType::I64])?;
                        }
                        (1 + len, false)
                    }
                    7 => {
                        trace!("parse_op_i64_trunc_sat_f64_u");
                        if !*unreachable {
                            instrs.push(Instr {
                                op: vm::op_i64_trunc_sat_f64_u,
                            });
                            checker.op(&[ValType::F64], &[ValType::I64])?;
                        }
                        (1 + len, false)
                    }
                    8 => {
                        let (len2, idx) = self.parse_u32()?;
                        let op = self.reader.read_exact_one()?;
                        if op != 0 {
                            Err(WasmParserError::InvalidInstruction([
                                0xFC, 8, idx as u8, op,
                            ]))?;
                        }
                        trace!("op_mem_init");
                        assert_memory(self.mems)?;
                        assert_data_idx(idx, data_count_section)?;

                        if !*unreachable {
                            instrs.push(Instr {
                                op: vm::op_mem_init,
                            });
                            instrs.push(Instr {
                                operand: Operand { u32: idx },
                            });
                            checker.op(&[ValType::I32, ValType::I32, ValType::I32], &[])?;
                        }
                        (2 + len + len2, false)
                    }
                    9 => {
                        let (len2, idx) = self.parse_u32()?;
                        trace!("op_data_drop");
                        assert_memory(self.mems)?;
                        assert_data_idx(idx, data_count_section)?;
                        //FIXME: do nothing
                        (1 + len + len2, false)
                    }
                    10 => {
                        let op = self.reader.read_exact_one()?;
                        if op != 0 {
                            Err(WasmParserError::InvalidInstruction([0xFC, 10, op, 0x00]))?;
                        }
                        let op = self.reader.read_exact_one()?;
                        if op != 0 {
                            Err(WasmParserError::InvalidInstruction([0xFC, 10, 0x00, op]))?;
                        }
                        trace!("op_mem_copy");
                        assert_memory(self.mems)?;
                        if !*unreachable {
                            instrs.push(Instr {
                                op: vm::op_mem_copy,
                            });
                            checker.op(&[ValType::I32, ValType::I32, ValType::I32], &[])?;
                        }
                        (3 + len, false)
                    }
                    11 => {
                        let op = self.reader.read_exact_one()?;
                        if op != 0 {
                            Err(WasmParserError::InvalidInstruction([0xFC, 11, op, 0x00]))?;
                        }
                        assert_memory(self.mems)?;
                        trace!("parse_op_mem_fill");
                        if !*unreachable {
                            instrs.push(Instr {
                                op: vm::op_mem_fill,
                            });
                            checker.op(&[ValType::I32, ValType::I32, ValType::I32], &[])?;
                        }
                        (2 + len, false)
                    }
                    12 => {
                        let (len2, elemidx) = self.parse_u32()?;
                        let (len3, tableidx) = self.parse_u32()?;
                        let elem = self
                            .elems
                            .get(elemidx as usize)
                            .ok_or(WasmParserError::UnknownElement)?;

                        validate_active_elem(self.tables, tableidx, elem.kind)?;
                        trace!("parse_op_table_init");
                        if !*unreachable {
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
                        }
                        (1 + len + len2 + len3, false)
                    }
                    13 => {
                        let (len2, elemidx) = self.parse_u32()?;
                        if self.elems.get(elemidx as usize).is_none() {
                            Err(WasmParserError::UnknownElement)?;
                        }
                        trace!("parse_op_elem_drop");
                        if !*unreachable {
                            instrs.push(Instr {
                                op: vm::op_elem_drop,
                            });
                            instrs.push(Instr {
                                operand: Operand { u32: elemidx },
                            });
                        }

                        (1 + len + len2, false)
                    }
                    14 => {
                        let (len2, tableidx) = self.parse_u32()?;
                        let (len3, tableidx2) = self.parse_u32()?;

                        trace!("parse_op_table_copy");
                        if !*unreachable {
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
                        }
                        (1 + len + len2 + len3, false)
                    }
                    _ => Err(WasmParserError::InvalidInstruction([
                        0xFC, next as u8, 0x00, 0x00,
                    ]))?,
                }
            }
            0xC0 => {
                trace!("parse_op_i32_extend8_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_extend8_s,
                    });
                    checker.binary_op(ValType::I32)?;
                }
                (1, false)
            }
            0xC1 => {
                trace!("parse_op_i32_extend16_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_extend16_s,
                    });
                    checker.binary_op(ValType::I32)?;
                }
                (1, false)
            }
            0xC2 => {
                trace!("parse_op_i64_extend8_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_extend8_s,
                    });
                    checker.binary_op(ValType::I64)?;
                }
                (1, false)
            }
            0xC3 => {
                trace!("parse_op_i64_extend16_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_extend16_s,
                    });
                    checker.binary_op(ValType::I64)?;
                }
                (1, false)
            }
            0xC4 => {
                trace!("parse_op_i64_extend32_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_extend32_s,
                    });
                    checker.binary_op(ValType::I64)?;
                }
                (1, false)
            }
            0xD0 => {
                trace!("parse_op_ref_null");
                let (len, t) = self.parse_reftype()?;

                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_ref_null,
                    });
                }
                checker.op(&[], &[t.into()])?;
                (1 + len, false)
            }
            0xD1 => {
                trace!("parse_op_ref_is_null");

                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_ref_is_null,
                    });
                }
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
                    Err(WasmParserError::InvalidFuncIdx(FuncIdx(idx)))?
                }
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_ref_func,
                    });
                    instrs.push(Instr {
                        operand: Operand { u32: idx },
                    });
                }
                checker.op(&[], &[ValType::FuncRef])?;
                (1 + len, false)
            }
            unknown => Err(WasmParserError::invalid_instruction1(unknown))?,
        })
    }
    pub fn parse_instrs(
        &mut self,
        data_count_section: &mut DataCountVerifier,
        instrs: &mut Vec<Instr>,
        checker: &mut TypeChecker,
        else_addr: &mut Option<u32>,
        unreachable: &mut bool,
        is_unreachable_if_block: bool,
    ) -> Result<usize> {
        let mut read_bytes = 0;
        loop {
            let (len, end) = self.parse_inst(
                data_count_section,
                instrs,
                checker,
                else_addr,
                unreachable,
                is_unreachable_if_block,
            )?;
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
        funcidx: FuncIdx,
        mems: &'a [MemType],
        functype: &'a FuncType,
        locals: &'a [Locals],
        globals: &'a [GlobalType],
        tables: &'a [TableType],
        elems: &'a [Elem],
    ) -> Self {
        Self {
            reader,
            elems,
            types,
            functions,
            funcidx,
            mems,
            functype,
            locals,
            globals,
            tables,
        }
    }
}
