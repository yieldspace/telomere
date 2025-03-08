use std::collections::VecDeque;

use thiserror::Error;
use tracing::trace;

use crate::{
    binary::BinaryReader,
    common::Operand,
    parser,
    runtime::vm::{special_function_return, WasmValue},
};

use super::{
    BlockType, CodeSection, Export, ExportDesc, ExportSection, Func, FuncIdx, FuncType,
    FunctionSection, Global, GlobalIdx, GlobalSection, GlobalType, Instr, Locals, MemArg, MemIdx,
    Module, Mut, ResultType, TableIdx, TypeIdx, TypeSection, ValType,
};

#[derive(Error, Debug)]
pub enum WasmParserError {
    #[error("invalid magic")]
    InvalidMagic([u8; 4]),
    #[error("invalid version")]
    InvalidVersion([u8; 4]),
    #[error("invalid leb128 encoding")]
    InvalidLeb128Encoding,
    #[error("invalid section size")]
    InvalidSectionSize,
    #[error("invalid function type signature: {0}")]
    InvalidFunctionTypeSignature(u8),
    #[error("invalid value type: {0}")]
    InvalidValueType(u8),
    #[error("invalid name encoding")]
    InvalidNameEncoding,
    #[error("invalid export desc: {0}")]
    InvalidExportDesc(u8),
    #[error("invalid instruction size: expected: {0:?}, actual: {1:?}")]
    InvalidInstructionSize(u32, u32),
    #[error("error from underlying layer")]
    IoError(#[from] std::io::Error),
    #[error("invalid instruction: {0:?}")]
    InvalidInstruction([u8; 4]),
    #[error("invalid blocktype")]
    InvalidBlockType(i64),
    #[error("invalid stack valtype: expected: {0:?}, actual: {1:?}")]
    InvalidStackValType(ValType, Option<ValType>),
    #[error("invalid stack valtype")]
    InvalidStackValTypeAny,
    #[error("invalid funcidx: {0:?}")]
    InvalidFuncIdx(FuncIdx),
    #[error("invalid typeidx: {0:?}")]
    InvalidTypeIdx(TypeIdx),
    #[error("invalid localidx: {0:?}")]
    InvalidLocalIndex(u32),
    #[error("invalid globalidx: {0:?}")]
    InvalidGlobalIndex(u32),
    #[error("invalid mut: {0:?}")]
    InvalidMut(u8),
    #[error("invalid global access")]
    InvalidGlobalAccess,
}
impl WasmParserError {
    pub fn invalid_instruction1(inst: u8) -> WasmParserError {
        WasmParserError::InvalidInstruction([inst, 0, 0, 0])
    }
}
pub type Result<R> = std::result::Result<R, WasmParserError>;
#[repr(u8)]
#[derive(Debug, PartialEq, Eq)]

enum WasmSectionType {
    Custom = 0,
    Type = 1,
    Import = 2,
    Function = 3,
    Table = 4,
    Memory = 5,
    Global = 6,
    Export = 7,
    Start = 8,
    Element = 9,
    Code = 10,
    Data = 11,
    DataCount = 12,
}
struct Leb128Parser<'a, R: BinaryReader> {
    reader: &'a mut R,
}
macro_rules! parse_leb128impl {
    ($name: ident, $t: ident, $is_signed: expr) => {
        fn $name(&mut self, bit_size: usize) -> Result<(usize, $t)> {
            let mut result: $t = 0;
            let mut read_bytes: usize = 0;
            let mut byte: u8;

            loop {
                byte = self.reader.read_exact_one()?;
                // Extract the lower 7 bits and shift them into the result.
                result |= ((byte & 0x7F) as $t) << (read_bytes * 7);
                read_bytes += 1;

                // If the most significant bit is not set, this is the final byte.
                if byte & 0x80 == 0 {
                    break;
                }

                // Check if the shift amount exceeds or equals the specified bit size.
                if (read_bytes * 7) >= bit_size {
                    return Err(WasmParserError::InvalidLeb128Encoding);
                }
            }

            // For signed numbers, perform sign extension if the sign bit (0x40) is set.
            if $is_signed && (read_bytes * 7) < bit_size && (byte & 0x40) != 0 {
                result |= (!0 as $t) << read_bytes * 7;
            }

            Ok((read_bytes, result))
        }
    };
    ($name: ident, $t: ident, $is_signed: expr) => {
        parse_leb128impl!($name, $t, $is_signed);
    };
    ($name: ident, $t: ident) => {
        parse_leb128impl!($name, $t, false);
    };
}
impl<'a, R: BinaryReader> Leb128Parser<'a, R> {
    fn new(reader: &'a mut R) -> Self {
        Self { reader }
    }
    parse_leb128impl!(parse_u32, u32);
    parse_leb128impl!(parse_i32, i32, true);
    parse_leb128impl!(parse_i64, i64, true);
}

pub struct WasmParser<'a, R: BinaryReader> {
    reader: &'a mut R,
}
fn assert_valtype(expected: ValType, actual: Option<ValType>) -> Result<()> {
    if let Some(actual) = actual {
        if expected == actual {
            Ok(())
        } else {
            Err(WasmParserError::InvalidStackValType(expected, Some(actual)))
        }
    } else {
        Err(WasmParserError::InvalidStackValType(expected, actual))
    }
}
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
fn get_global_addr(globals: &[Global], idx: u32) -> Result<(GlobalType, u32)> {
    let mut addr = 0;
    let mut i = 0;
    for global in globals {
        if i == idx {
            return Ok((global.0, addr));
        }
        i += 1;
        addr += global.0 .0.stack_size().u32();
    }
    Err(WasmParserError::InvalidGlobalIndex(idx))
}
impl<'a, R: BinaryReader> WasmParser<'a, R> {
    fn parse_u32(&mut self) -> Result<(usize, u32)> {
        Leb128Parser::new(self.reader).parse_u32(std::mem::size_of::<u32>() * 8)
    }
    fn parse_i32(&mut self) -> Result<(usize, i32)> {
        Leb128Parser::new(self.reader).parse_i32(std::mem::size_of::<i32>() * 8)
    }
    fn parse_i64(&mut self) -> Result<(usize, i64)> {
        Leb128Parser::new(self.reader).parse_i64(std::mem::size_of::<i64>() * 8)
    }
    fn parse_f32(&mut self) -> Result<(usize, f32)> {
        let v = self.reader.read_exact::<4>()?;
        Ok((4, f32::from_le_bytes(v)))
    }
    fn parse_f64(&mut self) -> Result<(usize, f64)> {
        let v = self.reader.read_exact::<8>()?;
        Ok((8, f64::from_le_bytes(v)))
    }
    fn parse_vec<V>(
        &mut self,
        mut f: impl FnMut(&mut Self) -> Result<(usize, V)>,
    ) -> Result<(usize, Vec<V>)> {
        let mut read_bytes = 0;

        let (len_len, len) = self.parse_u32()?;
        trace!("parse_vec: {len_len} {len}");
        read_bytes += len_len;
        let mut result = Vec::new();
        for _i in 0..len {
            let (len, v) = f(self)?;
            result.push(v);
            read_bytes += len;
        }
        Ok((read_bytes, result))
    }
    fn parse_byte(&mut self) -> Result<(usize, u8)> {
        Ok((1, self.reader.read_exact_one()?))
    }
    fn parse_name(&mut self) -> Result<(usize, String)> {
        let (len, name) = self.parse_vec(Self::parse_byte)?;
        Ok((
            len,
            String::from_utf8(name).map_err(|_| WasmParserError::InvalidNameEncoding)?,
        ))
    }
    fn skip_section(&mut self, size: u32) -> Result<()> {
        for _idx in 0..size {
            self.reader.read_exact_one()?;
        }
        Ok(())
    }

    fn parse_typeidx(&mut self) -> Result<(usize, TypeIdx)> {
        let (len, v) = self.parse_u32()?;
        Ok((len, TypeIdx(v)))
    }
    fn parse_valtype(&mut self) -> Result<(usize, ValType)> {
        let v = self.reader.read_exact_one()?;
        let ty = match v {
            0x7f => ValType::I32,
            0x7e => ValType::I64,
            0x7d => ValType::F32,
            0x7c => ValType::F64,
            0x7b => ValType::V128,
            0x70 => ValType::FuncRef,
            0x6f => ValType::ExternRef,
            unknown => Err(WasmParserError::InvalidValueType(unknown))?,
        };
        Ok((1, ty))
    }
    fn parse_result_type(&mut self) -> Result<(usize, ResultType)> {
        let (len, v) = self.parse_vec(Self::parse_valtype)?;
        Ok((len, ResultType(v)))
    }

    fn parse_functype(&mut self) -> Result<(usize, FuncType)> {
        let mut read_bytes = 0;
        let signature = self.reader.read_exact_one()?;
        trace!("parse_functype: {signature}");
        read_bytes += 1;
        if signature != 0x60 {
            Err(WasmParserError::InvalidFunctionTypeSignature(signature))?
        }
        let (len, input) = self.parse_result_type()?;
        trace!("parse_functype: {len} {input:?}");

        read_bytes += len;
        let (len, output) = self.parse_result_type()?;
        read_bytes += len;
        trace!("parse_functype: {len} {output:?}");

        Ok((read_bytes, FuncType(input, output)))
    }
    fn parse_exportdesc(&mut self) -> Result<(usize, ExportDesc)> {
        let mut read_bytes = 0;
        let (len, ty) = self.parse_byte()?;
        read_bytes += len;
        let (len, idx) = self.parse_u32()?;
        let desc = match ty {
            0x00 => ExportDesc::Func(FuncIdx(idx)),
            0x01 => ExportDesc::Table(TableIdx(idx)),
            0x02 => ExportDesc::Mem(MemIdx(idx)),
            0x03 => ExportDesc::Global(GlobalIdx(idx)),
            unknown => Err(WasmParserError::InvalidExportDesc(unknown))?,
        };
        read_bytes += len;
        Ok((read_bytes, desc))
    }

    fn parse_global_type(&mut self) -> Result<(usize, GlobalType)> {
        let (len, vt) = self.parse_valtype()?;
        let m = match self.reader.read_exact_one()? {
            0x00 => Mut::Const,
            0x01 => Mut::Var,
            unknown => Err(WasmParserError::InvalidMut(unknown))?,
        };
        Ok((1 + len, GlobalType(vt, m)))
    }
    fn parse_global_init(&mut self) -> Result<(usize, WasmValue)> {
        let v = self.reader.read_exact_one()?;
        let r = match v {
            0x41 => {
                let (len, operand) = self.parse_i32()?;
                (2 + len, WasmValue::I32(operand))
            }
            _ => Err(WasmParserError::invalid_instruction1(v))?,
        };
        let end_inst = self.reader.read_exact_one()?;
        if 0x0B != end_inst {
            Err(WasmParserError::invalid_instruction1(end_inst))?;
        }
        Ok(r)
    }
    fn parse_global(&mut self) -> Result<(usize, Global)> {
        let (len, gt) = self.parse_global_type()?;
        let (len2, init) = self.parse_global_init()?;
        Ok((len + len2, Global(gt, init)))
    }

    fn parse_export(&mut self) -> Result<(usize, Export)> {
        let mut read_bytes = 0;
        let (len, name) = self.parse_name()?;
        read_bytes += len;
        let (len, desc) = self.parse_exportdesc()?;
        read_bytes += len;
        Ok((read_bytes, Export(name, desc)))
    }
    fn parse_locals(&mut self) -> Result<(usize, Locals)> {
        let (len, n) = self.parse_u32()?;
        let (len2, t) = self.parse_valtype()?;
        Ok((len + len2, Locals { n, t }))
    }
    fn parse_block_type(&mut self) -> Result<(usize, BlockType)> {
        let (len, v) = Leb128Parser::new(self.reader).parse_i64(33)?;
        let t = if v < 0 {
            if len != 1 {
                Err(WasmParserError::InvalidBlockType(v))?
            }
            match v {
                -1 => BlockType::ValType(ValType::I32),
                -2 => BlockType::ValType(ValType::I64),
                -3 => BlockType::ValType(ValType::F32),
                -4 => BlockType::ValType(ValType::F64),
                -16 => BlockType::ValType(ValType::FuncRef),
                -17 => BlockType::ValType(ValType::ExternRef),
                -64 => BlockType::Void,
                _ => Err(WasmParserError::InvalidBlockType(v))?,
            }
        } else {
            BlockType::TypeIdx(TypeIdx(v as u32))
        };
        Ok((len, t))
    }
    fn parse_memarg(&mut self) -> Result<(usize, MemArg)> {
        let (len, align) = self.parse_u32()?;
        let (len2, offset) = self.parse_u32()?;
        Ok((len + len2, MemArg { align, offset }))
    }
    fn parse_inst(
        &mut self,
        type_section: &TypeSection,
        function_section: &FunctionSection,
        functype: &FuncType,
        locals: &[Locals],
        globals: &[Global],
        instrs: &mut Vec<Instr>,
        types: &mut Vec<ValType>,
        block_types_idxs: &mut VecDeque<(BlockType, usize)>,
        else_addr: &mut Option<u32>,
        unreachable: &mut bool,
    ) -> Result<(usize, bool)> {
        let v = self.reader.read_exact_one()?;
        use crate::runtime::vm;

        Ok(match v {
            0x0F => {
                trace!("parse_op_return");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_return });
                }
                (1, false)
            }
            0x0B => {
                trace!("parse_op_end");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_end });
                    *unreachable = false;
                }
                (1, true)
            }

            0x5e => {
                trace!("parse_op_f32_gt");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f32_gt });
                    assert_valtype(ValType::F32, types.pop())?;
                    assert_valtype(ValType::F32, types.pop())?;
                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x6A => {
                trace!("parse_op_i32_add");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i32_add });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_valtype(ValType::I32, types.pop())?;
                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x7C => {
                trace!("parse_op_i64_add");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i64_add });
                    assert_valtype(ValType::I64, types.pop())?;
                    assert_valtype(ValType::I64, types.pop())?;
                    types.push(ValType::I64);
                }
                (1, false)
            }
            0x00 => {
                trace!("parse_op_unreachable");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_unreachable,
                    });
                    *unreachable = true;
                }
                (1, false)
            }
            0x01 => (1, false),
            0x02 => {
                trace!("parse_op_block");
                let (len, blocktype) = self.parse_block_type()?;
                let mut unreachable = *unreachable;

                instrs.push(Instr { op: vm::op_block });
                instrs.push(Instr {
                    operand: Operand { u32: 0xFAFAFAFA },
                });
                let index = instrs.len() - 1;
                let before_stack_len = types.len();
                let mut block_input_size = 0;
                block_types_idxs.push_front((blocktype, before_stack_len));
                match blocktype {
                    BlockType::TypeIdx(idx) => {
                        let ty = type_section
                            .get(idx)
                            .ok_or_else(|| WasmParserError::InvalidTypeIdx(idx))?;
                        for ty in ty.0.stack_pop_iter() {
                            block_input_size += 1;
                            assert_valtype(*ty, types.pop())?;
                        }
                        for ty in ty.0.iter() {
                            types.push(*ty);
                        }
                    }
                    _ => {}
                }
                let len2 = self.parse_instrs(
                    type_section,
                    function_section,
                    functype,
                    locals,
                    globals,
                    instrs,
                    types,
                    block_types_idxs,
                    else_addr,
                    &mut unreachable,
                )?;
                block_types_idxs.pop_front();
                instrs[index].operand = Operand {
                    jump_addr: instrs.len() as u32,
                };
                let after_stack_len = types.len();
                let mut block_output_size = 0;
                match blocktype {
                    BlockType::Void => {}
                    BlockType::TypeIdx(idx) => {
                        let ty = type_section
                            .get(idx)
                            .ok_or_else(|| WasmParserError::InvalidTypeIdx(idx))?;

                        for ty in ty.1.stack_pop_iter() {
                            block_output_size += 1;
                            assert_valtype(*ty, types.pop())?;
                        }
                        for ty in ty.1.iter() {
                            types.push(*ty);
                        }
                    }
                    BlockType::ValType(ty) => {
                        assert_valtype(ty, types.last().copied())?;
                        block_output_size += 1;
                    }
                }
                trace!("{before_stack_len} - {block_input_size} == {after_stack_len} - {block_output_size}");
                if before_stack_len + block_output_size != after_stack_len + block_input_size {
                    Err(WasmParserError::InvalidStackValTypeAny)?
                }
                (1 + len + len2, false)
            }
            0x03 => {
                trace!("parse_op_loop");
                let (len, blocktype) = self.parse_block_type()?;
                let mut unreachable = *unreachable;
                instrs.push(Instr { op: vm::op_loop });
                instrs.push(Instr {
                    operand: Operand { u32: 0xFBFBFBFB },
                });
                let index = instrs.len() - 1;
                let before_stack_len = types.len();
                let len2 = self.parse_instrs(
                    type_section,
                    function_section,
                    functype,
                    locals,
                    globals,
                    instrs,
                    types,
                    block_types_idxs,
                    else_addr,
                    &mut unreachable,
                )?;
                instrs[index].operand = Operand {
                    jump_addr: instrs.len() as u32,
                };
                let mut after_stack_len = types.len();

                match blocktype {
                    BlockType::Void => {}
                    BlockType::TypeIdx(idx) => {
                        let ty = type_section
                            .get(idx)
                            .ok_or_else(|| WasmParserError::InvalidTypeIdx(idx))?;
                        for ty in ty.1.stack_pop_iter() {
                            after_stack_len -= 1;
                            assert_valtype(*ty, types.pop())?;
                        }
                        for ty in ty.1.iter() {
                            types.push(*ty);
                        }
                    }
                    BlockType::ValType(ty) => {
                        after_stack_len -= 1;
                        assert_valtype(ty, types.last().copied())?;
                    }
                }

                trace!("{before_stack_len} {after_stack_len}");
                if before_stack_len != after_stack_len {
                    Err(WasmParserError::InvalidStackValTypeAny)?
                }
                (1 + len + len2, false)
            }
            0x04 => {
                trace!("parse_op_if");
                let (len, blocktype) = self.parse_block_type()?;
                let mut unreachable = *unreachable;

                instrs.push(Instr { op: vm::op_if });
                instrs.push(Instr {
                    operand: Operand {
                        jump_addr2: (0xFCFCFCFC, 0xFDFDFDFD),
                    },
                });
                assert_valtype(ValType::I32, types.pop())?;
                let index = instrs.len() - 1;
                let before_stack_len = types.len();
                let mut block_input_size = 0;
                block_types_idxs.push_front((blocktype, before_stack_len));
                match blocktype {
                    BlockType::TypeIdx(idx) => {
                        let ty = type_section
                            .get(idx)
                            .ok_or_else(|| WasmParserError::InvalidTypeIdx(idx))?;
                        for ty in ty.0.iter() {
                            block_input_size += 1;
                            assert_valtype(*ty, types.pop())?;
                        }
                        for ty in ty.0.iter() {
                            types.push(*ty);
                        }
                    }
                    _ => {}
                }
                let mut else_addr = None;
                let len2 = self.parse_instrs(
                    type_section,
                    function_section,
                    functype,
                    locals,
                    globals,
                    instrs,
                    types,
                    block_types_idxs,
                    &mut else_addr,
                    &mut unreachable,
                )?;
                block_types_idxs.pop_front();
                instrs[index].operand = Operand {
                    jump_addr2: (
                        instrs.len() as u32,
                        else_addr.unwrap_or_else(|| instrs.len() as u32),
                    ),
                };
                let after_stack_len = types.len();
                let mut block_output_size = 0;
                match blocktype {
                    BlockType::Void => {}
                    BlockType::TypeIdx(idx) => {
                        let ty = type_section
                            .get(idx)
                            .ok_or_else(|| WasmParserError::InvalidTypeIdx(idx))?;

                        for ty in ty.1.stack_pop_iter() {
                            block_output_size += 1;
                            assert_valtype(*ty, types.pop())?;
                        }
                        for ty in ty.1.iter() {
                            types.push(*ty);
                        }
                    }
                    BlockType::ValType(ty) => {
                        assert_valtype(ty, types.last().copied())?;
                        block_output_size += 1;
                    }
                }
                trace!("{before_stack_len} - {block_input_size} == {after_stack_len} - {block_output_size}");
                if before_stack_len + block_output_size != after_stack_len + block_input_size {
                    Err(WasmParserError::InvalidStackValTypeAny)?
                }

                (1 + len + len2, false)
            }
            0x05 => {
                trace!("parse_op_else");
                instrs.push(Instr { op: vm::op_else });
                *else_addr = Some(instrs.len() as u32);
                *unreachable = false;
                if let Some((blocktype, size)) = block_types_idxs.get(0) {
                    match blocktype {
                        BlockType::Void => {}
                        BlockType::TypeIdx(idx) => {
                            let ty = type_section
                                .get(*idx)
                                .ok_or_else(|| WasmParserError::InvalidTypeIdx(*idx))?;
                            for ty in ty.1.stack_pop_iter() {
                                assert_valtype(*ty, types.pop())?;
                            }
                        }
                        BlockType::ValType(ty) => {
                            assert_valtype(*ty, types.pop())?;
                        }
                    }
                    if *size != types.len() {
                        Err(WasmParserError::InvalidStackValTypeAny)?
                    }
                } else {
                    Err(WasmParserError::InvalidStackValTypeAny)?
                }
                (1, false)
            }
            0x0C => {
                let (len, idx) = self.parse_u32()?;
                trace!("parse_op_br: {idx}");
                *unreachable = true;
                instrs.push(Instr { op: vm::op_br });
                instrs.push(Instr {
                    operand: Operand { u32: idx },
                });
                let mut return_size = 0;
                let mut consumed_size = 0;
                if let Some((blocktype, size)) = block_types_idxs.get(idx as usize) {
                    match blocktype {
                        BlockType::Void => {}
                        BlockType::TypeIdx(idx) => {
                            let ty = type_section
                                .get(*idx)
                                .ok_or_else(|| WasmParserError::InvalidTypeIdx(*idx))?;
                            consumed_size += ty.0.iter().count();

                            for ty in ty.1.stack_pop_iter() {
                                return_size += 1;
                                assert_valtype(*ty, types.pop())?;
                            }
                            for ty in ty.1.iter() {
                                types.push(*ty);
                            }
                        }
                        BlockType::ValType(ty) => {
                            return_size += 1;
                            assert_valtype(*ty, types.last().copied())?;
                        }
                    }
                    if *size + return_size != types.len() + consumed_size {
                        Err(WasmParserError::InvalidStackValTypeAny)?
                    }
                } else {
                    Err(WasmParserError::InvalidStackValTypeAny)?
                }
                (1 + len, false)
            }
            0x0D => {
                let (len, idx) = self.parse_u32()?;
                trace!("parse_op_br_if: {}", idx);

                if !*unreachable {
                    assert_valtype(ValType::I32, types.pop())?;
                    instrs.push(Instr { op: vm::op_br_if });
                    instrs.push(Instr {
                        operand: Operand { u32: idx },
                    });
                }
                (1 + len, false)
            }
            0x0E => {
                trace!("parse_op_br_table");
                let (len, idxs) = self.parse_vec(Self::parse_u32)?;
                let (len2, default_idx) = self.parse_u32()?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_br_table,
                    });
                    instrs.push(Instr {
                        operand: Operand {
                            u32: idxs.len() as u32,
                        },
                    });
                    for idx in idxs {
                        instrs.push(Instr {
                            operand: Operand { u32: idx },
                        });
                    }
                    instrs.push(Instr {
                        operand: Operand { u32: default_idx },
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                }
                (1 + len + len2, false)
            }
            0x10 => {
                trace!("parse_op_call");
                let (len, idx) = self.parse_u32()?;
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_call });
                    instrs.push(Instr {
                        operand: Operand { u32: idx },
                    });
                    let typeidx = function_section
                        .get(FuncIdx(idx))
                        .ok_or_else(|| WasmParserError::InvalidFuncIdx(FuncIdx(idx)))?;
                    let ty = type_section
                        .get(typeidx)
                        .ok_or_else(|| WasmParserError::InvalidTypeIdx(TypeIdx(idx)))?;
                    for ty in ty.0.stack_pop_iter() {
                        assert_valtype(*ty, types.pop())?;
                    }
                    for ty in ty.1.iter() {
                        types.push(*ty);
                    }
                }
                (1 + len, false)
            }
            0x11 => {
                trace!("parse_op_call_indirect");
                let (len, typeidx) = self.parse_u32()?;
                let (len2, tableidx) = self.parse_u32()?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_call_indirect,
                    });
                    instrs.push(Instr {
                        operand: Operand { u32: tableidx },
                    });
                    assert_valtype(ValType::I32, types.pop())?;

                    let ty = type_section
                        .get(TypeIdx(typeidx))
                        .ok_or_else(|| WasmParserError::InvalidTypeIdx(TypeIdx(typeidx)))?;
                    for ty in ty.0.stack_pop_iter() {
                        assert_valtype(*ty, types.pop())?;
                    }
                    for ty in ty.1.iter() {
                        types.push(*ty);
                    }
                }
                (1 + len + len2, false)
            }
            0x1A => {
                trace!("parse_op_drop");
                if !*unreachable {
                    if let Some(x) = types.pop() {
                        instrs.push(Instr { op: vm::op_drop });
                        instrs.push(Instr {
                            operand: Operand {
                                drop_size: x.stack_size().usize(),
                            },
                        });
                    } else {
                        Err(WasmParserError::InvalidStackValTypeAny)?
                    }
                }
                (1, false)
            }
            0x1B => {
                trace!("parse_op_select");
                if !*unreachable {
                    let x = if let Some(first) = types.pop() {
                        assert_valtype(first, types.pop())?;
                        first
                    } else {
                        Err(WasmParserError::InvalidStackValTypeAny)?
                    };
                    assert_valtype(ValType::I32, types.pop())?;
                    types.push(x);
                    instrs.push(Instr { op: vm::op_select });
                    instrs.push(Instr {
                        operand: Operand {
                            select: x.stack_size().usize(),
                        },
                    });
                }
                (1, false)
            }
            0x20 => {
                trace!("parse_op_local_get");
                let (len, idx) = self.parse_u32()?;
                if !*unreachable {
                    function_section.get(FuncIdx(idx));
                    let (ty, addr) = get_local_addr(&functype.0, locals, idx)?;
                    match ty.stack_size() {
                        crate::parser::core::ValueSize::Byte4 => instrs.push(Instr {
                            op: vm::op_local_get4,
                        }),
                        crate::parser::core::ValueSize::Byte8 => instrs.push(Instr {
                            op: vm::op_local_get8,
                        }),
                        crate::parser::core::ValueSize::Byte16 => todo!(),
                    }
                    instrs.push(Instr {
                        operand: Operand { local_addr: addr },
                    });
                    types.push(ty);
                }
                (1 + len, false)
            }
            0x21 => {
                trace!("parse_op_local_set");
                let (len, idx) = self.parse_u32()?;
                if !*unreachable {
                    function_section.get(FuncIdx(idx));
                    let (ty, addr) = get_local_addr(&functype.0, locals, idx)?;
                    match ty.stack_size() {
                        crate::parser::core::ValueSize::Byte4 => instrs.push(Instr {
                            op: vm::op_local_set4,
                        }),
                        crate::parser::core::ValueSize::Byte8 => instrs.push(Instr {
                            op: vm::op_local_set8,
                        }),
                        crate::parser::core::ValueSize::Byte16 => todo!(),
                    }
                    assert_valtype(ty, types.pop())?;
                    instrs.push(Instr {
                        operand: Operand { local_addr: addr },
                    });
                }
                (1 + len, false)
            }
            0x22 => {
                trace!("parse_op_local_tee");
                let (len, idx) = self.parse_u32()?;
                if !*unreachable {
                    function_section.get(FuncIdx(idx));
                    let (ty, addr) = get_local_addr(&functype.0, locals, idx)?;
                    match ty.stack_size() {
                        crate::parser::core::ValueSize::Byte4 => instrs.push(Instr {
                            op: vm::op_local_tee4,
                        }),
                        crate::parser::core::ValueSize::Byte8 => instrs.push(Instr {
                            op: vm::op_local_tee8,
                        }),
                        crate::parser::core::ValueSize::Byte16 => todo!(),
                    }
                    assert_valtype(ty, types.pop())?;
                    types.push(ty);
                    instrs.push(Instr {
                        operand: Operand { local_addr: addr },
                    });
                }
                (1 + len, false)
            }
            0x23 => {
                trace!("parse_op_global_get");

                let (len, idx) = self.parse_u32()?;
                if !*unreachable {
                    let (ty, addr) = get_global_addr(globals, idx)?;
                    match ty.0.stack_size() {
                        crate::parser::core::ValueSize::Byte4 => instrs.push(Instr {
                            op: vm::op_global_get4,
                        }),
                        crate::parser::core::ValueSize::Byte8 => instrs.push(Instr {
                            op: vm::op_global_get8,
                        }),
                        crate::parser::core::ValueSize::Byte16 => todo!(),
                    }
                    types.push(ty.0);
                    instrs.push(Instr {
                        operand: Operand { u32: addr },
                    });
                }
                (1 + len, false)
            }
            0x24 => {
                trace!("parse_op_global_set");
                let (len, idx) = self.parse_u32()?;
                if !*unreachable {
                    function_section.get(FuncIdx(idx));
                    let (ty, addr) = get_global_addr(globals, idx)?;
                    if ty.1 != Mut::Var {
                        Err(WasmParserError::InvalidGlobalAccess)?
                    }
                    match ty.0.stack_size() {
                        crate::parser::core::ValueSize::Byte4 => instrs.push(Instr {
                            op: vm::op_global_set4,
                        }),
                        crate::parser::core::ValueSize::Byte8 => instrs.push(Instr {
                            op: vm::op_global_set8,
                        }),
                        crate::parser::core::ValueSize::Byte16 => todo!(),
                    }
                    assert_valtype(ty.0, types.pop())?;
                    instrs.push(Instr {
                        operand: Operand { u32: addr },
                    });
                }
                (1 + len, false)
            }
            0x28 => {
                trace!("parse_op_i32_load");
                let (len, memarg) = self.parse_memarg()?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_load,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    types.push(ValType::I32);
                }
                (1 + len, false)
            }
            0x36 => {
                trace!("parse_op_i32_store");
                let (len, memarg) = self.parse_memarg()?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_store,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_valtype(ValType::I32, types.pop())?;
                }
                (1 + len, false)
            }
            0x40 => {
                trace!("parse_op_mem_glow");
                let next = self.reader.read_exact_one()?;
                if next != 0 {
                    Err(WasmParserError::InvalidInstruction([0x40, next, 0, 0]))?
                }
                if !*unreachable {
                    assert_valtype(ValType::I32, types.pop())?;
                    types.push(ValType::I32);
                    instrs.push(Instr {
                        op: vm::op_mem_glow,
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
                    types.push(ValType::I32);
                }
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
                    types.push(ValType::I64);
                }
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
                    types.push(ValType::F32);
                }
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
                    types.push(ValType::F64);
                }
                (1 + len, false)
            }
            0x45 => {
                trace!("parse_op_i32_eqz");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i32_eqz });
                    assert_valtype(ValType::I32, types.pop())?;
                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x46 => {
                trace!("parse_op_i32_eq");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i32_eq });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_valtype(ValType::I32, types.pop())?;

                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x47 => {
                trace!("parse_op_i32_ne");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i32_ne });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_valtype(ValType::I32, types.pop())?;

                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x4D => {
                trace!("parse_op_i32_le_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_le_u,
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_valtype(ValType::I32, types.pop())?;

                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x50 => {
                trace!("parse_op_i64_eqz");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i64_eqz });
                    assert_valtype(ValType::I64, types.pop())?;
                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x58 => {
                trace!("parse_op_i64_le_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_le_u,
                    });
                    assert_valtype(ValType::I64, types.pop())?;
                    assert_valtype(ValType::I64, types.pop())?;
                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x68 => {
                trace!("parse_op_i32_ctz");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i32_ctz });
                    assert_valtype(ValType::I32, types.pop())?;
                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x69 => {
                trace!("parse_op_i32_popcnt");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_popcnt,
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x70 => {
                trace!("parse_op_i32_rem_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_rem_u,
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_valtype(ValType::I32, types.pop())?;
                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x6B => {
                trace!("parse_op_i32_sub");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i32_sub });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_valtype(ValType::I32, types.pop())?;
                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x6C => {
                trace!("parse_op_i32_mul");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i32_mul });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_valtype(ValType::I32, types.pop())?;
                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x7D => {
                trace!("parse_op_i64_sub");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i64_sub });
                    assert_valtype(ValType::I64, types.pop())?;
                    assert_valtype(ValType::I64, types.pop())?;
                    types.push(ValType::I64);
                }
                (1, false)
            }
            0x7E => {
                trace!("parse_op_i64_mul");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i64_mul });
                    assert_valtype(ValType::I64, types.pop())?;
                    assert_valtype(ValType::I64, types.pop())?;
                    types.push(ValType::I64);
                }
                (1, false)
            }
            0x91 => {
                trace!("parse_op_f32_sqrt");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f32_sqrt,
                    });
                    assert_valtype(ValType::F32, types.pop())?;
                    types.push(ValType::F32);
                }
                (1, false)
            }
            0xAC => {
                trace!("parse_op_i64_extend_i32_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_extend_i32_s,
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    types.push(ValType::I64);
                }
                (1, false)
            }
            unknown => Err(WasmParserError::invalid_instruction1(unknown))?,
        })
    }
    fn parse_instrs(
        &mut self,
        type_section: &TypeSection,
        function_section: &FunctionSection,
        functype: &FuncType,
        locals: &[Locals],
        globals: &[Global],
        instrs: &mut Vec<Instr>,
        types: &mut Vec<ValType>,
        block_types_idxs: &mut VecDeque<(BlockType, usize)>,
        else_addr: &mut Option<u32>,
        unreachable: &mut bool,
    ) -> Result<usize> {
        let mut read_bytes = 0;
        loop {
            let (len, end) = self.parse_inst(
                type_section,
                function_section,
                functype,
                locals,
                globals,
                instrs,
                types,
                block_types_idxs,
                else_addr,
                unreachable,
            )?;
            read_bytes += len;
            if end {
                return Ok(read_bytes);
            }
        }
    }
    fn parse_code_inner(
        &mut self,
        type_section: &TypeSection,
        function_section: &FunctionSection,
        global_section: &GlobalSection,
        functype: &FuncType,
        size: u32,
    ) -> Result<Func> {
        let (len, locals) = self.parse_vec(&Self::parse_locals)?;
        let mut instrs = Vec::new();
        let mut types = Vec::new();
        let mut block_types_idxs = VecDeque::new();
        let mut unreachable = false;
        let mut else_addr = None;
        let len2 = self.parse_instrs(
            type_section,
            function_section,
            functype,
            &locals,
            &global_section.0,
            &mut instrs,
            &mut types,
            &mut block_types_idxs,
            &mut else_addr,
            &mut unreachable,
        )?;
        for ty in functype.1.stack_pop_iter() {
            assert_valtype(*ty, types.pop())?;
        }
        if !types.is_empty() {
            Err(WasmParserError::InvalidStackValTypeAny)?
        }
        if len + len2 != size as usize {
            Err(WasmParserError::InvalidInstructionSize(
                size,
                (len + len2) as u32,
            ))?
        }
        instrs.push(Instr {
            op: special_function_return,
        });
        Ok(Func {
            locals,
            expr: instrs,
        })
    }
    fn parse_code(
        &mut self,
        type_section: &TypeSection,
        function_section: &FunctionSection,
        global_section: &GlobalSection,
        funcidx: FuncIdx,
    ) -> Result<(usize, Func)> {
        trace!("parse_code: {funcidx:?}");
        let (len, size) = self.parse_u32()?;
        let typeidx = function_section
            .get(funcidx)
            .ok_or_else(|| WasmParserError::InvalidFuncIdx(funcidx))?;
        let functype = type_section
            .get(typeidx)
            .ok_or_else(|| WasmParserError::InvalidTypeIdx(typeidx))?;
        let func = self.parse_code_inner(
            type_section,
            function_section,
            global_section,
            functype,
            size,
        )?;
        Ok((len + size as usize, func))
    }

    fn parse_section_type(&mut self) -> Result<WasmSectionType> {
        let kind = self.reader.read_exact_one()?;
        trace!("{kind}");
        use WasmSectionType::*;
        Ok(match kind {
            0 => Custom,
            1 => Type,
            2 => Import,
            3 => Function,
            4 => Table,
            5 => Memory,
            6 => Global,
            7 => Export,
            8 => Start,
            9 => Element,
            10 => Code,
            11 => Data,
            12 => DataCount,
            _ => Custom,
        })
    }

    fn parse_type_section(&mut self, size: u32) -> Result<TypeSection> {
        let (len, funcs) = self.parse_vec(&Self::parse_functype)?;
        if len != size as usize {
            Err(WasmParserError::InvalidSectionSize)?
        }
        Ok(TypeSection(funcs))
    }

    fn parse_function_section(&mut self, size: u32) -> Result<FunctionSection> {
        let (len, funcs) = self.parse_vec(&Self::parse_typeidx)?;
        if len != size as usize {
            Err(WasmParserError::InvalidSectionSize)?
        }
        Ok(FunctionSection(funcs))
    }
    fn parse_global_section(&mut self, size: u32) -> Result<GlobalSection> {
        let (len, globals) = self.parse_vec(&Self::parse_global)?;
        if len != size as usize {
            Err(WasmParserError::InvalidSectionSize)?
        }
        Ok(GlobalSection(globals))
    }
    fn parse_export_section(&mut self, size: u32) -> Result<ExportSection> {
        let (len, exports) = self.parse_vec(&Self::parse_export)?;
        if len != size as usize {
            Err(WasmParserError::InvalidSectionSize)?
        }
        Ok(ExportSection(exports))
    }

    fn parse_code_section(
        &mut self,
        type_section: &TypeSection,
        function_section: &FunctionSection,
        global_section: &GlobalSection,
        size: u32,
    ) -> Result<CodeSection> {
        let mut idx = 0;
        let (len, codes) = self.parse_vec(|me| {
            let r = Self::parse_code(
                me,
                type_section,
                function_section,
                global_section,
                parser::core::FuncIdx(idx),
            )?;
            idx += 1;
            Ok(r)
        })?;
        if len != size as usize {
            Err(WasmParserError::InvalidSectionSize)?
        }
        Ok(CodeSection(codes))
    }

    fn parse_section_body<V>(
        &mut self,
        mut f: impl FnMut(&mut Self, u32) -> Result<V>,
    ) -> Result<V> {
        let (_len, size) = self.parse_u32()?;
        f(self, size)
    }
    fn parse_magic(&mut self) -> Result<()> {
        let magic = self.reader.read_exact::<4>()?;
        if matches!(&magic, &[0x00, 0x61, 0x73, 0x6d]) {
            Ok(())
        } else {
            Err(WasmParserError::InvalidMagic(magic))
        }
    }
    fn parse_version(&mut self) -> Result<()> {
        let version = self.reader.read_exact::<4>()?;
        if matches!(&version, &[0x01, 0x00, 0x00, 0x00]) {
            Ok(())
        } else {
            Err(WasmParserError::InvalidVersion(version))
        }
    }

    pub fn new(reader: &'a mut R) -> Self {
        Self { reader }
    }
    pub fn parse_module(&mut self) -> Result<Module> {
        self.parse_magic()?;
        self.parse_version()?;
        let mut type_section: Option<TypeSection> = None;
        let mut function_section: Option<FunctionSection> = None;
        let mut export_section: Option<ExportSection> = None;
        let mut global_section: Option<GlobalSection> = None;

        loop {
            match self.parse_section_type()? {
                WasmSectionType::Custom => {
                    let (_, size) = self.parse_u32()?;
                    self.skip_section(size)?;
                }
                WasmSectionType::Type => {
                    trace!("type section");
                    type_section = Some(self.parse_section_body(Self::parse_type_section)?);
                }
                WasmSectionType::Import => todo!(),
                WasmSectionType::Function => {
                    function_section = Some(self.parse_section_body(Self::parse_function_section)?);
                }
                WasmSectionType::Table => {
                    let (_, size) = self.parse_u32()?;
                    self.skip_section(size)?;
                }
                WasmSectionType::Memory => {
                    let (_, size) = self.parse_u32()?;
                    self.skip_section(size)?;
                }
                WasmSectionType::Global => {
                    global_section = Some(self.parse_section_body(Self::parse_global_section)?);
                }
                WasmSectionType::Export => {
                    export_section = Some(self.parse_section_body(Self::parse_export_section)?);
                }
                WasmSectionType::Start => {
                    let (_, size) = self.parse_u32()?;
                    self.skip_section(size)?;
                }
                WasmSectionType::Element => {
                    let (_, size) = self.parse_u32()?;
                    self.skip_section(size)?;
                }
                WasmSectionType::Code => {
                    let type_section = type_section.unwrap();
                    let function_section = function_section.unwrap();
                    let global_section = global_section.unwrap_or_else(|| GlobalSection(vec![]));
                    let code_section = self.parse_section_body(|me, size| {
                        Self::parse_code_section(
                            me,
                            &type_section,
                            &function_section,
                            &global_section,
                            size,
                        )
                    })?;
                    return Ok(Module {
                        fts: type_section,
                        xs: function_section,
                        gs: global_section,
                        exs: export_section.unwrap_or_else(|| ExportSection(vec![])),
                        codes: code_section,
                    });
                }
                WasmSectionType::Data => {
                    let (_, size) = self.parse_u32()?;
                    self.skip_section(size)?;
                }
                WasmSectionType::DataCount => {
                    let (_, size) = self.parse_u32()?;
                    self.skip_section(size)?;
                }
            }
        }
    }
}
