use std::collections::VecDeque;

use thiserror::Error;
use tracing::trace;

use crate::{
    binary::BinaryReader,
    common::{
        BlockReturn, BlockType, CodeSection, Data, DataCountVerifier, DataMode, DataSection, Elem,
        ElemMode, ElementSection, Export, ExportDesc, ExportSection, Func, FuncIdx, FuncType,
        FunctionSection, Global, GlobalIdx, GlobalSection, GlobalType, Import, ImportDesc,
        ImportSection, Instr, Limits, Locals, LoopParam, MemArg, MemIdx, MemType, MemorySection,
        Mut, Operand, RefType, ResultType, Table, TableIdx, TableSection, TableType, TypeIdx,
        TypeSection, ValType, ValueSize, WasmValue,
    },
    runtime::vm,
    Module,
};
use crate::parser::leb128::Leb128Parser;

#[derive(Error, Debug)]
pub enum WasmParserError {
    #[error("invalid magic: {0:?}")]
    InvalidMagic([u8; 4]),
    #[error("invalid version: {0:?}")]
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
    #[error("invalid const instruction: {0}")]
    InvalidConstInstruction(u8),
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
    #[error("invalid elem kind: {0}")]
    InvalidElemKind(u8),
    #[error("invalid element section size: {0}")]
    InvalidElementSectionType(u32),
    #[error("invalid table index: {0}")]
    InvalidTableIndex(u32),
    #[error("invalid table type: {0}")]
    InvalidTableType(u32),
    #[error("multiple memory")]
    MultipleMemory,
    #[error("invalid import desc: {0}")]
    InvalidImportDesc(u8),
    #[error("invalid data kind: {0}")]
    InvalidDataKind(u32),
    #[error("invalid memidx: {0}")]
    InvalidMemIdx(u32),
    #[error("invalid memory size: {0:?}")]
    InvalidMemorySize(Limits),
    #[error("invalid alignment: {0}")]
    InvalidAlignment(u32),
    #[error("invalid dataidx: {0}")]
    InvalidDataIdx(u32),
    #[error("invalid data section count")]
    InvalidDataSectionCount,
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
fn assert_type_stack_size(
    types: &Vec<ValType>,
    blocks: &VecDeque<(BlockKind, BlockType, u32)>,
) -> Result<()> {
    let expected = blocks
        .front()
        .ok_or_else(|| WasmParserError::InvalidStackValTypeAny)?
        .2 as usize;
    let actual = types.len();
    if expected <= actual {
        Ok(())
    } else {
        Err(WasmParserError::InvalidStackValTypeAny)
    }
}
fn validate_br_table_types(
    idx: u32,
    type_section: &TypeSection,
    types: &mut Vec<ValType>,
    blocks: &VecDeque<(BlockKind, BlockType, u32)>,
) -> Result<u32> {
    let result_len = if let Some((kind, blocktype, _)) = blocks.get(idx as usize) {
        match kind {
            BlockKind::Block | BlockKind::If => match blocktype {
                BlockType::Void => {
                    assert_type_stack_size(types, blocks)?;
                    0
                }
                BlockType::ValType(ty) => {
                    assert_valtype(*ty, types.pop())?;
                    assert_type_stack_size(types, blocks)?;

                    types.push(*ty);
                    1
                }
                BlockType::TypeIdx(idx) => {
                    let ty = type_section
                        .get(*idx)
                        .ok_or_else(|| WasmParserError::InvalidTypeIdx(*idx))?;
                    for ty in ty.1.stack_pop_iter() {
                        assert_valtype(*ty, types.pop())?;
                    }
                    assert_type_stack_size(types, blocks)?;

                    for ty in ty.1.iter() {
                        types.push(*ty);
                    }
                    ty.1.iter().count() as u32
                }
            },
            BlockKind::Loop => match blocktype {
                BlockType::Void | BlockType::ValType(_) => {
                    assert_type_stack_size(types, blocks)?;

                    // ok
                    0
                }
                BlockType::TypeIdx(idx) => {
                    let ty = type_section
                        .get(*idx)
                        .ok_or_else(|| WasmParserError::InvalidTypeIdx(*idx))?;
                    for ty in ty.0.stack_pop_iter() {
                        assert_valtype(*ty, types.pop())?;
                    }
                    assert_type_stack_size(types, blocks)?;
                    for ty in ty.0.iter() {
                        types.push(*ty);
                    }
                    ty.1.iter().count() as u32
                }
            },
        }
    } else {
        Err(WasmParserError::InvalidStackValTypeAny)?
    };
    Ok(result_len)
}
fn assert_memory(sec: &MemorySection) -> Result<()> {
    if sec.0.len() <= 0 {
        Err(WasmParserError::InvalidMemIdx(0))?;
    }
    Ok(())
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
enum BlockKind {
    Block,
    If,
    Loop,
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
    fn parse_import_desc(&mut self) -> Result<(usize, ImportDesc)> {
        let ty = self.reader.read_exact_one()?;
        Ok(match ty {
            0x00 => {
                let (len, idx) = self.parse_u32()?;
                (1 + len, ImportDesc::TypeIdx(TypeIdx(idx)))
            }
            0x01 => {
                let (len, tt) = self.parse_table_type()?;
                (1 + len, ImportDesc::TableType(tt))
            }
            0x02 => {
                let (len, mem) = self.parse_memtype()?;
                (1 + len, ImportDesc::MemType(mem))
            }
            0x03 => {
                let (len, gt) = self.parse_global_type()?;
                (1 + len, ImportDesc::GlobalType(gt))
            }
            unknown => Err(WasmParserError::InvalidImportDesc(unknown))?,
        })
    }
    fn parse_import(&mut self) -> Result<(usize, Import)> {
        let (len, module) = self.parse_name()?;
        let (len2, name) = self.parse_name()?;
        let (len3, desc) = self.parse_import_desc()?;
        Ok((len + len2 + len3, Import { desc, module, name }))
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

    fn parse_ref_type(&mut self) -> Result<(usize, RefType)> {
        let v = self.reader.read_exact_one()?;
        Ok((
            1,
            match v {
                0x70 => RefType::FuncRef,
                0x6f => RefType::ExternRef,
                unknown => Err(WasmParserError::InvalidValueType(unknown))?,
            },
        ))
    }
    fn parse_table_type(&mut self) -> Result<(usize, TableType)> {
        let (len, reftype) = self.parse_ref_type()?;
        let (len2, limits) = self.parse_limits()?;
        Ok((len + len2, TableType { reftype, limits }))
    }
    fn parse_table(&mut self) -> Result<(usize, Table)> {
        let (len, tt) = self.parse_table_type()?;
        Ok((len, Table(tt)))
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

    fn parse_const_expr(&mut self) -> Result<(usize, WasmValue)> {
        let v = self.reader.read_exact_one()?;
        let r = match v {
            0x41 => {
                let (len, operand) = self.parse_i32()?;
                (2 + len, WasmValue::I32(operand))
            }
            0x42 => {
                let (len, operand) = self.parse_i64()?;
                (2 + len, WasmValue::I64(operand))
            }
            0x43 => {
                let (len, operand) = self.parse_f32()?;
                (2 + len, WasmValue::F32(operand))
            }
            0x44 => {
                let (len, operand) = self.parse_f64()?;
                (2 + len, WasmValue::F64(operand))
            }
            unknown => Err(WasmParserError::InvalidConstInstruction(unknown))?,
        };
        let end_inst = self.reader.read_exact_one()?;
        if 0x0B != end_inst {
            Err(WasmParserError::invalid_instruction1(end_inst))?;
        }
        Ok(r)
    }

    fn parse_global(&mut self) -> Result<(usize, Global)> {
        let (len, gt) = self.parse_global_type()?;
        let (len2, init) = self.parse_const_expr()?;
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

    fn parse_elem(&mut self, function_section: &FunctionSection) -> Result<(usize, Elem)> {
        let (len, kind) = self.parse_u32()?;
        let r = match kind {
            0 => {
                let (len2, offset) = self.parse_const_expr()?;
                let (len3, funcidx) = self.parse_vec(Self::parse_u32)?;
                for funcidx in &funcidx {
                    if function_section.get(FuncIdx(*funcidx)).is_none() {
                        Err(WasmParserError::InvalidFuncIdx(FuncIdx(*funcidx)))?;
                    }
                }
                (
                    len + len2 + len3,
                    Elem {
                        kind: RefType::FuncRef,
                        init: funcidx.iter().map(|v| *v).collect(),
                        mode: ElemMode::Active(TableIdx(0), offset),
                    },
                )
            }
            1 => {
                let elemkind = self.reader.read_exact_one()?;
                if elemkind != 0x00 {
                    Err(WasmParserError::InvalidElemKind(elemkind))?
                }
                let (len3, funcidx) = self.parse_vec(Self::parse_u32)?;
                for funcidx in &funcidx {
                    if function_section.get(FuncIdx(*funcidx)).is_none() {
                        Err(WasmParserError::InvalidFuncIdx(FuncIdx(*funcidx)))?;
                    }
                }
                (
                    len + 1 + len3,
                    Elem {
                        kind: RefType::FuncRef,
                        init: funcidx.iter().map(|v| *v).collect(),
                        mode: ElemMode::Passive,
                    },
                )
            }
            2 => {
                let (len2, tableidx) = self.parse_u32()?;
                let (len3, offset) = self.parse_const_expr()?;
                let elemkind = self.reader.read_exact_one()?;
                if elemkind != 0x00 {
                    Err(WasmParserError::InvalidElemKind(elemkind))?
                }
                let (len5, funcidx) = self.parse_vec(Self::parse_u32)?;
                for funcidx in &funcidx {
                    if function_section.get(FuncIdx(*funcidx)).is_none() {
                        Err(WasmParserError::InvalidFuncIdx(FuncIdx(*funcidx)))?;
                    }
                }
                (
                    len + len2 + len3 + 1 + len5,
                    Elem {
                        kind: RefType::FuncRef,
                        init: funcidx.iter().map(|v| *v).collect(),
                        mode: ElemMode::Active(TableIdx(tableidx), offset),
                    },
                )
            }
            3 => {
                let elemkind = self.reader.read_exact_one()?;
                if elemkind != 0x00 {
                    Err(WasmParserError::InvalidElemKind(elemkind))?
                }
                let (len3, funcidx) = self.parse_vec(Self::parse_u32)?;
                for funcidx in &funcidx {
                    if function_section.get(FuncIdx(*funcidx)).is_none() {
                        Err(WasmParserError::InvalidFuncIdx(FuncIdx(*funcidx)))?;
                    }
                }
                (
                    len + 1 + len3,
                    Elem {
                        kind: RefType::FuncRef,
                        init: funcidx.iter().map(|v| *v).collect(),
                        mode: ElemMode::Declarative,
                    },
                )
            }
            4..7 => {
                todo!()
            }
            unknown => Err(WasmParserError::InvalidElementSectionType(unknown))?,
        };
        Ok(r)
    }
    fn parse_limits(&mut self) -> Result<(usize, Limits)> {
        match self.reader.read_exact_one()? {
            0x00 => {
                let (len, min) = self.parse_u32()?;
                Ok((1 + len, Limits { min, max: None }))
            }
            0x01 => {
                let (len, min) = self.parse_u32()?;
                let (len2, max) = self.parse_u32()?;

                Ok((
                    1 + len + len2,
                    Limits {
                        min,
                        max: Some(max),
                    },
                ))
            }
            _ => todo!(),
        }
    }
    fn parse_memtype(&mut self) -> Result<(usize, MemType)> {
        let (len, limits) = self.parse_limits()?;
        if limits
            .max
            .map(|max| limits.min > max)
            .unwrap_or_else(|| false)
        {
            Err(WasmParserError::InvalidMemorySize(limits))?
        }
        if limits.min > 65536 {
            Err(WasmParserError::InvalidMemorySize(limits))?
        }
        if limits.max.map(|max| max > 65536).unwrap_or_else(|| false) {
            Err(WasmParserError::InvalidMemorySize(limits))?
        }
        Ok((len, MemType(limits)))
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
        type_section: &TypeSection,
        function_section: &FunctionSection,
        memory_section: &MemorySection,
        data_count_section: &mut DataCountVerifier,
        functype: &FuncType,
        locals: &[Locals],
        globals: &[Global],
        tables: &[Table],
        instrs: &mut Vec<Instr>,
        types: &mut Vec<ValType>,
        blocks: &mut VecDeque<(BlockKind, BlockType, u32)>,
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
                (1, false)
            }
            0x01 => (1, false),
            0x02 => {
                let (len, blocktype) = self.parse_block_type()?;
                trace!("parse_op_block: {blocktype:?}");

                let mut unreachable = *unreachable;

                instrs.push(Instr { op: vm::op_block });
                instrs.push(Instr {
                    operand: Operand {
                        jump_addr: 0xFAFAFAFA,
                    },
                });
                let index = instrs.len() - 1;
                let before_stack_len = types.len();
                let mut block_input_size: usize = 0;
                match blocktype {
                    BlockType::TypeIdx(idx) => {
                        let ty = type_section
                            .get(idx)
                            .ok_or_else(|| WasmParserError::InvalidTypeIdx(idx))?;
                        for ty in ty.0.stack_pop_iter() {
                            block_input_size += 1;
                            assert_valtype(*ty, types.pop())?;
                        }
                        assert_type_stack_size(types, blocks)?;
                        for ty in ty.0.iter() {
                            types.push(*ty);
                        }
                    }
                    _ => {}
                };
                let block_base_stack_len = before_stack_len - block_input_size;
                let block_base_stack_size = types[0..block_base_stack_len]
                    .iter()
                    .map(|v| v.stack_size().u32())
                    .sum();
                blocks.push_front((BlockKind::Block, blocktype, block_base_stack_len as u32));

                let len2 = self.parse_instrs(
                    type_section,
                    function_section,
                    memory_section,
                    data_count_section,
                    functype,
                    locals,
                    globals,
                    tables,
                    instrs,
                    types,
                    blocks,
                    else_addr,
                    &mut unreachable,
                    is_unreachable_if_block,
                )?;
                blocks.pop_front();
                instrs[index].operand.jump_addr = instrs.len() as u32;
                let mut return_size = 0;
                trace!("{types:?}");
                match blocktype {
                    BlockType::TypeIdx(idx) => {
                        let ty = type_section
                            .get(idx)
                            .ok_or_else(|| WasmParserError::InvalidTypeIdx(idx))?;
                        for ty in ty.1.stack_pop_iter() {
                            return_size += ty.stack_size().u32();
                            assert_valtype(*ty, types.pop())?;
                        }
                        assert_type_stack_size(types, blocks)?;
                        if unreachable {
                            types.truncate(block_base_stack_len);
                        } else if types.len() != block_base_stack_len {
                            Err(WasmParserError::InvalidStackValTypeAny)?;
                        }

                        for ty in ty.1.iter() {
                            types.push(*ty);
                        }
                    }
                    BlockType::ValType(ty) => {
                        assert_valtype(ty, types.pop())?;
                        assert_type_stack_size(types, blocks)?;
                        return_size += ty.stack_size().u32();
                        if unreachable {
                            types.truncate(block_base_stack_len);
                        } else if types.len() != block_base_stack_len {
                            Err(WasmParserError::InvalidStackValTypeAny)?;
                        }
                        types.push(ty);
                    }
                    BlockType::Void => {
                        if unreachable {
                            types.truncate(block_base_stack_len);
                        } else if types.len() != block_base_stack_len {
                            Err(WasmParserError::InvalidStackValTypeAny)?;
                        }
                    }
                }
                instrs.push(Instr {
                    op: vm::special_block_return,
                });
                instrs.push(Instr {
                    operand: Operand {
                        block_return: BlockReturn {
                            stack_top: block_base_stack_size,
                            return_size,
                        },
                    },
                });
                (1 + len + len2, false)
            }
            0x03 => {
                let (len, blocktype) = self.parse_block_type()?;
                trace!("parse_op_loop: {blocktype:?}");

                let mut unreachable = *unreachable;
                instrs.push(Instr { op: vm::op_loop });
                instrs.push(Instr {
                    operand: Operand {
                        jump_addr: (instrs.len() - 1) as u32,
                    },
                });

                let before_stack_len = types.len();
                let mut block_input_len = 0;
                let mut block_input_size = 0;
                match blocktype {
                    BlockType::TypeIdx(idx) => {
                        let ty = type_section
                            .get(idx)
                            .ok_or_else(|| WasmParserError::InvalidTypeIdx(idx))?;
                        trace!("ty: {ty:?}");

                        for ty in ty.0.stack_pop_iter() {
                            block_input_len += 1;
                            block_input_size += ty.stack_size().u32();
                            assert_valtype(*ty, types.pop())?;
                        }
                        assert_type_stack_size(types, blocks)?;
                        for ty in ty.0.iter() {
                            types.push(*ty);
                        }
                    }
                    _ => {}
                }
                let block_base_stack_len = before_stack_len - block_input_len;
                let block_base_stack_size = types[0..block_base_stack_len]
                    .iter()
                    .map(|v| v.stack_size().u32())
                    .sum();
                instrs.push(Instr {
                    operand: Operand {
                        loop_param: LoopParam {
                            stack_top: block_base_stack_size,
                            param_size: block_input_size as u32,
                        },
                    },
                });
                blocks.push_front((BlockKind::Loop, blocktype, block_base_stack_len as u32));

                let len2 = self.parse_instrs(
                    type_section,
                    function_section,
                    memory_section,
                    data_count_section,
                    functype,
                    locals,
                    globals,
                    tables,
                    instrs,
                    types,
                    blocks,
                    else_addr,
                    &mut unreachable,
                    is_unreachable_if_block,
                )?;

                blocks.pop_front();
                tracing::trace!("{block_base_stack_len} {blocktype:?} {types:?}");

                let return_size = match blocktype {
                    BlockType::Void => {
                        if unreachable {
                            types.truncate(block_base_stack_len);
                        }
                        if !unreachable && types.len() != block_base_stack_len {
                            Err(WasmParserError::InvalidStackValTypeAny)?;
                        }
                        0
                    }
                    BlockType::TypeIdx(idx) => {
                        let ty = type_section
                            .get(idx)
                            .ok_or_else(|| WasmParserError::InvalidTypeIdx(idx))?;
                        if !unreachable {
                            for ty in ty.1.stack_pop_iter() {
                                assert_valtype(*ty, types.pop())?;
                            }
                            assert_type_stack_size(types, blocks)?;
                        }
                        if unreachable {
                            types.truncate(block_base_stack_len);
                        }
                        if !unreachable && types.len() != block_base_stack_len {
                            Err(WasmParserError::InvalidStackValTypeAny)?;
                        }
                        for ty in ty.1.iter() {
                            types.push(*ty);
                        }

                        ty.1.iter().map(|v| v.stack_size().u32()).sum()
                    }
                    BlockType::ValType(ty) => {
                        if !unreachable {
                            assert_valtype(ty, types.pop())?;
                        }
                        assert_type_stack_size(types, blocks)?;
                        if unreachable {
                            types.truncate(block_base_stack_len);
                        }
                        if !unreachable && types.len() != block_base_stack_len {
                            Err(WasmParserError::InvalidStackValTypeAny)?;
                        }
                        types.push(ty);

                        ty.stack_size().u32()
                    }
                };
                tracing::trace!("{types:?}");
                instrs.push(Instr {
                    op: vm::special_block_return,
                });
                instrs.push(Instr {
                    operand: Operand {
                        block_return: BlockReturn {
                            stack_top: block_base_stack_size,
                            return_size,
                        },
                    },
                });
                (1 + len + len2, false)
            }
            0x04 => {
                trace!("parse_op_if");
                let (len, blocktype) = self.parse_block_type()?;
                let mut unreachable = *unreachable;
                let is_unreachable_if_block = unreachable;
                let mut base_stack_len = 0;

                if !is_unreachable_if_block {
                    instrs.push(Instr { op: vm::op_if });
                    instrs.push(Instr {
                        operand: Operand {
                            jump_addr2: (0xFCFCFCFC, 0xFDFDFDFD),
                        },
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(types, blocks)?;
                    let before_stack_len = types.len();
                    base_stack_len = before_stack_len;
                    match blocktype {
                        BlockType::TypeIdx(idx) => {
                            let ty = type_section
                                .get(idx)
                                .ok_or_else(|| WasmParserError::InvalidTypeIdx(idx))?;
                            for ty in ty.0.iter() {
                                assert_valtype(*ty, types.pop())?;
                            }
                            base_stack_len = types.len();
                            assert_type_stack_size(types, blocks)?;
                            for ty in ty.0.iter() {
                                types.push(*ty);
                            }
                        }
                        _ => {}
                    }
                    blocks.push_front((BlockKind::If, blocktype, base_stack_len as u32));
                }

                let index = instrs.len() - 1;
                let mut else_addr = None;
                let len2 = self.parse_instrs(
                    type_section,
                    function_section,
                    memory_section,
                    data_count_section,
                    functype,
                    locals,
                    globals,
                    tables,
                    instrs,
                    types,
                    blocks,
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
                    blocks.pop_front();
                }
                match blocktype {
                    BlockType::Void => {
                        if unreachable {
                            types.truncate(base_stack_len);
                        } else {
                            if types.len() != base_stack_len {
                                Err(WasmParserError::InvalidStackValTypeAny)?;
                            }
                        }
                    }
                    BlockType::TypeIdx(idx) => {
                        let ty = type_section
                            .get(idx)
                            .ok_or_else(|| WasmParserError::InvalidTypeIdx(idx))?;

                        if !unreachable {
                            if else_addr.is_none() {
                                for ty in ty.1.stack_pop_iter() {
                                    assert_valtype(*ty, types.pop())?;
                                }
                                assert_type_stack_size(types, blocks)?;
                                for ty in ty.0.iter() {
                                    types.push(*ty);
                                }
                            }
                            for ty in ty.1.stack_pop_iter() {
                                assert_valtype(*ty, types.pop())?;
                            }
                            assert_type_stack_size(types, blocks)?;
                            if types.len() != base_stack_len {
                                Err(WasmParserError::InvalidStackValTypeAny)?;
                            }
                        } else {
                            types.truncate(base_stack_len);
                        }

                        for ty in ty.1.iter() {
                            types.push(*ty);
                        }
                    }
                    BlockType::ValType(ty) => {
                        if !unreachable {
                            if else_addr.is_none() {
                                assert_valtype(ty, types.pop())?;
                                assert_type_stack_size(types, blocks)?;
                            }
                            assert_valtype(ty, types.pop())?;
                            assert_type_stack_size(types, blocks)?;
                            if types.len() != base_stack_len {
                                Err(WasmParserError::InvalidStackValTypeAny)?;
                            }
                        } else {
                            types.truncate(base_stack_len);
                        }

                        types.push(ty);
                    }
                }

                (1 + len + len2, false)
            }
            0x05 => {
                trace!("parse_op_else");
                *unreachable = is_unreachable_if_block;
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_else });
                    *else_addr = Some(instrs.len() as u32);
                    if let Some((BlockKind::If, blocktype, block_base_stack_len)) = blocks.get(0) {
                        match blocktype {
                            BlockType::Void => {
                                if types.len() != *block_base_stack_len as usize {
                                    Err(WasmParserError::InvalidStackValTypeAny)?;
                                }
                            }
                            BlockType::TypeIdx(idx) => {
                                let ty = type_section
                                    .get(*idx)
                                    .ok_or_else(|| WasmParserError::InvalidTypeIdx(*idx))?;
                                for ty in ty.1.stack_pop_iter() {
                                    assert_valtype(*ty, types.pop())?;
                                }
                                assert_type_stack_size(types, blocks)?;
                                if types.len() != *block_base_stack_len as usize {
                                    Err(WasmParserError::InvalidStackValTypeAny)?;
                                }
                                for ty in ty.0.iter() {
                                    types.push(*ty);
                                }
                            }
                            BlockType::ValType(ty) => {
                                assert_valtype(*ty, types.pop())?;
                                assert_type_stack_size(types, blocks)?;
                                if types.len() != *block_base_stack_len as usize {
                                    Err(WasmParserError::InvalidStackValTypeAny)?;
                                }
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
                *unreachable = true;

                instrs.push(Instr { op: vm::op_br });
                instrs.push(Instr {
                    operand: Operand { u32: idx },
                });
                if let Some((kind, blocktype, block_base_stack_len)) = blocks.get(idx as usize) {
                    match kind {
                        BlockKind::Block | BlockKind::If => match blocktype {
                            BlockType::Void => {
                                // ok
                            }
                            BlockType::ValType(ty) => {
                                assert_valtype(*ty, types.pop())?;
                                assert_type_stack_size(types, blocks)?;

                                types.truncate(*block_base_stack_len as usize);
                                types.push(*ty);
                            }
                            BlockType::TypeIdx(idx) => {
                                let ty = type_section
                                    .get(*idx)
                                    .ok_or_else(|| WasmParserError::InvalidTypeIdx(*idx))?;
                                for ty in ty.1.stack_pop_iter() {
                                    assert_valtype(*ty, types.pop())?;
                                }
                                assert_type_stack_size(types, blocks)?;
                                types.truncate(*block_base_stack_len as usize);

                                for ty in ty.1.iter() {
                                    types.push(*ty);
                                }
                            }
                        },
                        BlockKind::Loop => match blocktype {
                            BlockType::Void | BlockType::ValType(_) => {
                                assert_type_stack_size(types, blocks)?;
                                types.truncate(*block_base_stack_len as usize);
                                // ok
                            }
                            BlockType::TypeIdx(idx) => {
                                let ty = type_section
                                    .get(*idx)
                                    .ok_or_else(|| WasmParserError::InvalidTypeIdx(*idx))?;
                                let mut arity = 0;
                                for ty in ty.0.stack_pop_iter() {
                                    assert_valtype(*ty, types.pop())?;
                                    arity += 1;
                                }
                                assert_type_stack_size(types, blocks)?;
                                types.truncate((block_base_stack_len - arity) as usize);
                                for ty in ty.0.iter() {
                                    types.push(*ty);
                                }
                            }
                        },
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
                    if let Some((kind, blocktype, _base_stack_len)) = blocks.get(idx as usize) {
                        match kind {
                            BlockKind::Block | BlockKind::If => {
                                match blocktype {
                                    BlockType::Void => {
                                        // ok
                                    }
                                    BlockType::ValType(ty) => {
                                        assert_valtype(*ty, types.pop())?;
                                        //
                                        assert_type_stack_size(&types, &blocks)?;
                                        types.push(*ty);
                                    }
                                    BlockType::TypeIdx(idx) => {
                                        let ty = type_section
                                            .get(*idx)
                                            .ok_or_else(|| WasmParserError::InvalidTypeIdx(*idx))?;
                                        for ty in ty.1.stack_pop_iter() {
                                            assert_valtype(*ty, types.pop())?;
                                        }
                                        assert_type_stack_size(&types, &blocks)?;
                                        for ty in ty.1.iter() {
                                            types.push(*ty);
                                        }
                                    }
                                }
                            }
                            BlockKind::Loop => {
                                // do nothing
                            }
                        }
                    } else {
                        Err(WasmParserError::InvalidStackValTypeAny)?;
                    }
                }
                (1 + len, false)
            }
            0x0E => {
                let (len, idxs) = self.parse_vec(Self::parse_u32)?;
                let (len2, default_idx) = self.parse_u32()?;
                trace!("parse_op_br_table: {idxs:?} {default_idx} {types:?}");

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
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;
                    let result_len =
                        validate_br_table_types(default_idx, type_section, types, blocks)?;
                    for idx in idxs {
                        if result_len != validate_br_table_types(idx, type_section, types, blocks)?
                        {
                            Err(WasmParserError::InvalidStackValTypeAny)?;
                        }
                    }
                    *unreachable = true;
                }
                (1 + len + len2, false)
            }
            0x0F => {
                trace!("parse_op_return");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_return });
                    for ty in functype.1.stack_pop_iter() {
                        assert_valtype(*ty, types.pop())?;
                    }
                    assert_type_stack_size(types, blocks)?;
                    types.truncate(0);
                    for ty in functype.1.iter() {
                        types.push(*ty);
                    }
                    *unreachable = true;
                }
                (1, false)
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
                    assert_type_stack_size(&types, &blocks)?;
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
                    if tables.len() <= tableidx as usize {
                        Err(WasmParserError::InvalidTableIndex(tableidx))?;
                    }
                    if tables[tableidx as usize].0.reftype != RefType::FuncRef {
                        Err(WasmParserError::InvalidTableType(tableidx))?;
                    }
                    instrs.push(Instr {
                        op: vm::op_call_indirect,
                    });
                    instrs.push(Instr {
                        operand: Operand { u32: tableidx },
                    });
                    instrs.push(Instr {
                        operand: Operand { u32: typeidx },
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;
                    let ty = type_section
                        .get(TypeIdx(typeidx))
                        .ok_or_else(|| WasmParserError::InvalidTypeIdx(TypeIdx(typeidx)))?;
                    for ty in ty.0.stack_pop_iter() {
                        assert_valtype(*ty, types.pop())?;
                    }
                    assert_type_stack_size(&types, &blocks)?;
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
                                drop_size: x.stack_size().u32(),
                            },
                        });
                    } else {
                        Err(WasmParserError::InvalidStackValTypeAny)?
                    }
                    assert_type_stack_size(&types, &blocks)?;
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
                    assert_type_stack_size(&types, &blocks)?;
                    types.push(x);
                    instrs.push(Instr { op: vm::op_select });
                    instrs.push(Instr {
                        operand: Operand {
                            select: x.stack_size().u32(),
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
                        ValueSize::Byte4 => instrs.push(Instr {
                            op: vm::op_local_set4,
                        }),
                        ValueSize::Byte8 => instrs.push(Instr {
                            op: vm::op_local_set8,
                        }),
                        ValueSize::Byte16 => todo!(),
                    }
                    assert_valtype(ty, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;
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
                        ValueSize::Byte4 => instrs.push(Instr {
                            op: vm::op_local_tee4,
                        }),
                        ValueSize::Byte8 => instrs.push(Instr {
                            op: vm::op_local_tee8,
                        }),
                        ValueSize::Byte16 => todo!(),
                    }
                    assert_valtype(ty, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;
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
                        ValueSize::Byte4 => instrs.push(Instr {
                            op: vm::op_global_get4,
                        }),
                        ValueSize::Byte8 => instrs.push(Instr {
                            op: vm::op_global_get8,
                        }),
                        ValueSize::Byte16 => todo!(),
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
                        ValueSize::Byte4 => instrs.push(Instr {
                            op: vm::op_global_set4,
                        }),
                        ValueSize::Byte8 => instrs.push(Instr {
                            op: vm::op_global_set8,
                        }),
                        ValueSize::Byte16 => todo!(),
                    }
                    assert_valtype(ty.0, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;
                    instrs.push(Instr {
                        operand: Operand { u32: addr },
                    });
                }
                (1 + len, false)
            }
            0x28 => {
                trace!("parse_op_i32_load");
                assert_memory(memory_section)?;
                let (len, memarg) = self.parse_memarg(4)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_load,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;
                    types.push(ValType::I32);
                }
                (1 + len, false)
            }
            0x29 => {
                trace!("parse_op_i64_load");
                assert_memory(memory_section)?;
                let (len, memarg) = self.parse_memarg(8)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_load,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;
                    types.push(ValType::I64);
                }
                (1 + len, false)
            }
            0x2A => {
                trace!("parse_op_f32_load");
                assert_memory(memory_section)?;
                let (len, memarg) = self.parse_memarg(4)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f32_load,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;
                    types.push(ValType::F32);
                }
                (1 + len, false)
            }
            0x2B => {
                trace!("parse_op_f64_load");
                assert_memory(memory_section)?;
                let (len, memarg) = self.parse_memarg(8)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f64_load,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;
                    types.push(ValType::F64);
                }
                (1 + len, false)
            }
            0x2C => {
                trace!("parse_op_i32_load8_s");
                assert_memory(memory_section)?;
                let (len, memarg) = self.parse_memarg(1)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_load8_s,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;
                    types.push(ValType::I32);
                }
                (1 + len, false)
            }
            0x2D => {
                trace!("parse_op_i32_load8_u");
                assert_memory(memory_section)?;
                let (len, memarg) = self.parse_memarg(1)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_load8_u,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;
                    types.push(ValType::I32);
                }
                (1 + len, false)
            }
            0x2E => {
                trace!("parse_op_i32_load16_s");
                assert_memory(memory_section)?;
                let (len, memarg) = self.parse_memarg(2)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_load16_s,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;
                    types.push(ValType::I32);
                }
                (1 + len, false)
            }
            0x2F => {
                trace!("parse_op_i32_load16_u");
                assert_memory(memory_section)?;
                let (len, memarg) = self.parse_memarg(2)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_load16_u,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;
                    types.push(ValType::I32);
                }
                (1 + len, false)
            }
            0x30 => {
                trace!("parse_op_i64_load8_s");
                assert_memory(memory_section)?;
                let (len, memarg) = self.parse_memarg(1)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_load8_s,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;
                    types.push(ValType::I64);
                }
                (1 + len, false)
            }
            0x31 => {
                trace!("parse_op_i64_load8_u");
                assert_memory(memory_section)?;
                let (len, memarg) = self.parse_memarg(1)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_load8_u,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;
                    types.push(ValType::I64);
                }
                (1 + len, false)
            }
            0x32 => {
                trace!("parse_op_i64_load16_s");
                assert_memory(memory_section)?;
                let (len, memarg) = self.parse_memarg(2)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_load16_s,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;
                    types.push(ValType::I64);
                }
                (1 + len, false)
            }
            0x33 => {
                trace!("parse_op_i64_load16_u");
                assert_memory(memory_section)?;
                let (len, memarg) = self.parse_memarg(2)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_load16_u,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;
                    types.push(ValType::I64);
                }
                (1 + len, false)
            }
            0x34 => {
                trace!("parse_op_i64_load32_s");
                assert_memory(memory_section)?;
                let (len, memarg) = self.parse_memarg(4)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_load32_s,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;
                    types.push(ValType::I64);
                }
                (1 + len, false)
            }
            0x35 => {
                trace!("parse_op_i64_load32_u");
                assert_memory(memory_section)?;
                let (len, memarg) = self.parse_memarg(4)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_load32_u,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;
                    types.push(ValType::I64);
                }
                (1 + len, false)
            }
            0x36 => {
                trace!("parse_op_i32_store");
                assert_memory(memory_section)?;
                let (len, memarg) = self.parse_memarg(4)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_store,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;
                }
                (1 + len, false)
            }
            0x37 => {
                trace!("parse_op_i64_store");
                assert_memory(memory_section)?;
                let (len, memarg) = self.parse_memarg(8)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_store,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    assert_valtype(ValType::I64, types.pop())?;
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;
                }
                (1 + len, false)
            }
            0x38 => {
                trace!("parse_op_f32_store");
                assert_memory(memory_section)?;
                let (len, memarg) = self.parse_memarg(4)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f32_store,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    assert_valtype(ValType::F32, types.pop())?;
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;
                }
                (1 + len, false)
            }
            0x39 => {
                trace!("parse_op_f64_store");
                assert_memory(memory_section)?;
                let (len, memarg) = self.parse_memarg(8)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_f64_store,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    assert_valtype(ValType::F64, types.pop())?;
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;
                }
                (1 + len, false)
            }
            0x3A => {
                trace!("parse_op_i32_store8");
                assert_memory(memory_section)?;
                let (len, memarg) = self.parse_memarg(1)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_store8,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;
                }
                (1 + len, false)
            }
            0x3B => {
                trace!("parse_op_i32_store16");
                assert_memory(memory_section)?;
                let (len, memarg) = self.parse_memarg(2)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_store16,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;
                }
                (1 + len, false)
            }
            0x3C => {
                trace!("parse_op_i64_store8");
                assert_memory(memory_section)?;
                let (len, memarg) = self.parse_memarg(1)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_store8,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    assert_valtype(ValType::I64, types.pop())?;
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;
                }
                (1 + len, false)
            }
            0x3D => {
                trace!("parse_op_i64_store16");
                assert_memory(memory_section)?;
                let (len, memarg) = self.parse_memarg(2)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_store16,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    assert_valtype(ValType::I64, types.pop())?;
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;
                }
                (1 + len, false)
            }
            0x3E => {
                trace!("parse_op_i64_store32");
                assert_memory(memory_section)?;
                let (len, memarg) = self.parse_memarg(4)?;
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_store32,
                    });
                    instrs.push(Instr {
                        operand: Operand { memarg },
                    });
                    assert_valtype(ValType::I64, types.pop())?;
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;
                }
                (1 + len, false)
            }
            0x3F => {
                trace!("parse_op_mem_size");
                let next = self.reader.read_exact_one()?;
                assert_memory(memory_section)?;
                if next != 0x00 {
                    Err(WasmParserError::InvalidInstruction([0x3F, next, 0, 0]))?
                }
                if !*unreachable {
                    types.push(ValType::I32);
                    instrs.push(Instr {
                        op: vm::op_mem_size,
                    });
                }
                (2, false)
            }
            0x40 => {
                trace!("parse_op_mem_grow");
                let next = self.reader.read_exact_one()?;
                if next != 0x00 {
                    Err(WasmParserError::InvalidInstruction([0x40, next, 0, 0]))?
                }
                assert_memory(memory_section)?;
                if !*unreachable {
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;
                    types.push(ValType::I32);
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
                    assert_type_stack_size(&types, &blocks)?;
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
                    assert_type_stack_size(&types, &blocks)?;

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
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x48 => {
                trace!("parse_op_i32_lt_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_lt_s,
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x49 => {
                trace!("parse_op_i32_lt_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_lt_u,
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x4C => {
                trace!("parse_op_i32_le_s");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_le_s,
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

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
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x4F => {
                trace!("parse_op_i32_ge_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_ge_u,
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x50 => {
                trace!("parse_op_i64_eqz");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i64_eqz });
                    assert_valtype(ValType::I64, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x54 => {
                trace!("parse_op_i64_lt_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_lt_u,
                    });
                    assert_valtype(ValType::I64, types.pop())?;
                    assert_valtype(ValType::I64, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x56 => {
                trace!("parse_op_i64_gt_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_gt_u,
                    });
                    assert_valtype(ValType::I64, types.pop())?;
                    assert_valtype(ValType::I64, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

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
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x5B => {
                trace!("parse_op_f32_eq");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f32_eq });
                    assert_valtype(ValType::F32, types.pop())?;
                    assert_valtype(ValType::F32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x5C => {
                trace!("parse_op_f32_ne");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f32_ne });
                    assert_valtype(ValType::F32, types.pop())?;
                    assert_valtype(ValType::F32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x5D => {
                trace!("parse_op_f32_lt");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f32_lt });
                    assert_valtype(ValType::F32, types.pop())?;
                    assert_valtype(ValType::F32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x5E => {
                trace!("parse_op_f32_gt");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f32_gt });
                    assert_valtype(ValType::F32, types.pop())?;
                    assert_valtype(ValType::F32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x5F => {
                trace!("parse_op_f32_le");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f32_le });
                    assert_valtype(ValType::F32, types.pop())?;
                    assert_valtype(ValType::F32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x61 => {
                trace!("parse_op_f64_eq");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f64_eq });
                    assert_valtype(ValType::F64, types.pop())?;
                    assert_valtype(ValType::F64, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x65 => {
                trace!("parse_op_f64_le");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f64_le });
                    assert_valtype(ValType::F64, types.pop())?;
                    assert_valtype(ValType::F64, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x67 => {
                trace!("parse_op_i32_clz");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i32_clz });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x68 => {
                trace!("parse_op_i32_ctz");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i32_ctz });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

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
                    assert_type_stack_size(&types, &blocks)?;

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
                    assert_type_stack_size(&types, &blocks)?;

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
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x6E => {
                trace!("parse_op_i32_div_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_div_u,
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

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
                    assert_type_stack_size(types, blocks)?;
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
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x71 => {
                trace!("parse_op_i32_and");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i32_and });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::I32);
                }
                (1, false)
            }
            0x7A => {
                trace!("parse_op_i64_ctz");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i64_ctz });
                    assert_valtype(ValType::I64, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::I64);
                }
                (1, false)
            }
            0x7C => {
                trace!("parse_op_i64_add");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i64_add });
                    assert_valtype(ValType::I64, types.pop())?;
                    assert_valtype(ValType::I64, types.pop())?;
                    assert_type_stack_size(types, blocks)?;
                    types.push(ValType::I64);
                }
                (1, false)
            }
            0x7D => {
                trace!("parse_op_i64_sub");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_i64_sub });
                    assert_valtype(ValType::I64, types.pop())?;
                    assert_valtype(ValType::I64, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

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
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::I64);
                }
                (1, false)
            }
            0x8C => {
                trace!("parse_op_f32_neg");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f32_neg });
                    assert_valtype(ValType::F32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::F32);
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
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::F32);
                }
                (1, false)
            }
            0x92 => {
                trace!("parse_op_f32_add");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f32_add });
                    assert_valtype(ValType::F32, types.pop())?;
                    assert_valtype(ValType::F32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::F32);
                }
                (1, false)
            }
            0x93 => {
                trace!("parse_op_f32_sub");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f32_sub });
                    assert_valtype(ValType::F32, types.pop())?;
                    assert_valtype(ValType::F32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::F32);
                }
                (1, false)
            }
            0x94 => {
                trace!("parse_op_f32_mul");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f32_mul });
                    assert_valtype(ValType::F32, types.pop())?;
                    assert_valtype(ValType::F32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::F32);
                }
                (1, false)
            }
            0x95 => {
                trace!("parse_op_f32_div");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f32_div });
                    assert_valtype(ValType::F32, types.pop())?;
                    assert_valtype(ValType::F32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::F32);
                }
                (1, false)
            }
            0x9A => {
                trace!("parse_op_f64_neg");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f64_neg });
                    assert_valtype(ValType::F64, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::F64);
                }
                (1, false)
            }
            0xA0 => {
                trace!("parse_op_f64_add");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f64_add });
                    assert_valtype(ValType::F64, types.pop())?;
                    assert_valtype(ValType::F64, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::F64);
                }
                (1, false)
            }
            0xA1 => {
                trace!("parse_op_f64_sub");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f64_sub });
                    assert_valtype(ValType::F64, types.pop())?;
                    assert_valtype(ValType::F64, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::F64);
                }
                (1, false)
            }
            0xA2 => {
                trace!("parse_op_f64_mul");
                if !*unreachable {
                    instrs.push(Instr { op: vm::op_f64_mul });
                    assert_valtype(ValType::F64, types.pop())?;
                    assert_valtype(ValType::F64, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::F64);
                }
                (1, false)
            }
            0xA7 => {
                trace!("parse_op_i32_wrap_i64");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i32_wrap_i64,
                    });
                    assert_valtype(ValType::I64, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::I32);
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
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::I64);
                }
                (1, false)
            }
            0xAD => {
                trace!("parse_op_i64_extend_i32_u");
                if !*unreachable {
                    instrs.push(Instr {
                        op: vm::op_i64_extend_i32_u,
                    });
                    assert_valtype(ValType::I32, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::I64);
                }
                (1, false)
            }
            0xBF => {
                trace!("parse_op_f64_reinterpret_i64");
                if !*unreachable {
                    assert_valtype(ValType::I64, types.pop())?;
                    assert_type_stack_size(&types, &blocks)?;

                    types.push(ValType::F64);
                }
                (1, false)
            }
            0xFC => {
                let (len, next) = self.parse_u32()?;
                match next {
                    8 => {
                        let (len2, idx) = self.parse_u32()?;
                        let op = self.reader.read_exact_one()?;
                        if op != 0 {
                            Err(WasmParserError::InvalidInstruction([
                                0xFC, 8, idx as u8, op,
                            ]))?;
                        }
                        trace!("op_mem_init");
                        assert_memory(memory_section)?;
                        assert_data_idx(idx, data_count_section)?;

                        if !*unreachable {
                            instrs.push(Instr {
                                op: vm::op_mem_init,
                            });
                            instrs.push(Instr {
                                operand: Operand { u32: idx },
                            });
                            assert_valtype(ValType::I32, types.pop())?;
                            assert_valtype(ValType::I32, types.pop())?;
                            assert_valtype(ValType::I32, types.pop())?;
                            assert_type_stack_size(&types, &blocks)?;
                        }
                        (2 + len + len2, false)
                    }
                    9 => {
                        let (len2, idx) = self.parse_u32()?;
                        trace!("op_data_drop");
                        assert_memory(memory_section)?;
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
                        assert_memory(memory_section)?;
                        if !*unreachable {
                            instrs.push(Instr {
                                op: vm::op_mem_copy,
                            });
                            assert_valtype(ValType::I32, types.pop())?;
                            assert_valtype(ValType::I32, types.pop())?;
                            assert_valtype(ValType::I32, types.pop())?;
                            assert_type_stack_size(&types, &blocks)?;
                        }
                        (3 + len, false)
                    }
                    11 => {
                        let op = self.reader.read_exact_one()?;
                        if op != 0 {
                            Err(WasmParserError::InvalidInstruction([0xFC, 11, op, 0x00]))?;
                        }
                        assert_memory(memory_section)?;
                        trace!("parse_op_mem_fill");
                        if !*unreachable {
                            instrs.push(Instr {
                                op: vm::op_mem_fill,
                            });
                            assert_valtype(ValType::I32, types.pop())?;
                            assert_valtype(ValType::I32, types.pop())?;
                            assert_valtype(ValType::I32, types.pop())?;
                            assert_type_stack_size(&types, &blocks)?;
                        }
                        (2 + len, false)
                    }
                    _ => Err(WasmParserError::InvalidInstruction([
                        0xFC, next as u8, 0x00, 0x00,
                    ]))?,
                }
            }
            unknown => Err(WasmParserError::invalid_instruction1(unknown))?,
        })
    }
    fn parse_instrs(
        &mut self,
        type_section: &TypeSection,
        function_section: &FunctionSection,
        memory_section: &MemorySection,
        data_count_section: &mut DataCountVerifier,
        functype: &FuncType,
        locals: &[Locals],
        globals: &[Global],
        tables: &[Table],
        instrs: &mut Vec<Instr>,
        types: &mut Vec<ValType>,
        blocks: &mut VecDeque<(BlockKind, BlockType, u32)>,
        else_addr: &mut Option<u32>,
        unreachable: &mut bool,
        is_unreachable_if_block: bool,
    ) -> Result<usize> {
        let mut read_bytes = 0;
        loop {
            let (len, end) = self.parse_inst(
                type_section,
                function_section,
                memory_section,
                data_count_section,
                functype,
                locals,
                globals,
                tables,
                instrs,
                types,
                blocks,
                else_addr,
                unreachable,
                is_unreachable_if_block,
            )?;
            trace!("{types:?}");
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
        table_section: &TableSection,
        memory_section: &MemorySection,
        data_count_section: &mut DataCountVerifier,
        functype: &FuncType,
        typeidx: TypeIdx,
        size: u32,
    ) -> Result<Func> {
        let (len, locals) = self.parse_vec(&Self::parse_locals)?;
        let mut instrs = Vec::new();
        let mut types = Vec::new();
        let mut block_types_idxs = VecDeque::new();
        block_types_idxs.push_front((BlockKind::Block, BlockType::TypeIdx(typeidx), 0));
        let mut unreachable = false;
        let mut else_addr = None;
        let len2 = self.parse_instrs(
            type_section,
            function_section,
            memory_section,
            data_count_section,
            functype,
            &locals,
            &global_section.0,
            &table_section.0,
            &mut instrs,
            &mut types,
            &mut block_types_idxs,
            &mut else_addr,
            &mut unreachable,
            false,
        )?;
        trace!("function return");
        if !unreachable {
            for ty in functype.1.stack_pop_iter() {
                assert_valtype(*ty, types.pop())?;
            }
            if !types.is_empty() {
                Err(WasmParserError::InvalidStackValTypeAny)?
            }
        }
        if len + len2 != size as usize {
            Err(WasmParserError::InvalidInstructionSize(
                size,
                (len + len2) as u32,
            ))?
        }

        instrs.push(Instr {
            op: vm::special_function_return,
        });
        instrs.push(Instr {
            operand: Operand {
                drop_size: functype.1.iter().map(|v| v.stack_size().u32()).sum(),
            },
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
        table_section: &TableSection,
        memory_section: &MemorySection,
        data_count_section: &mut DataCountVerifier,
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
            table_section,
            memory_section,
            data_count_section,
            functype,
            typeidx,
            size,
        )?;
        Ok((len + size as usize, func))
    }
    fn parse_data(&mut self, memory_section: &MemorySection) -> Result<(usize, Data)> {
        let (len, kind) = self.parse_u32()?;
        match kind {
            0x00 => {
                if memory_section.0.len() <= 0 {
                    Err(WasmParserError::InvalidMemIdx(0))?;
                }
                let (len2, offset) = self.parse_const_expr()?;
                let (len3, bytes) = self.parse_vec(Self::parse_byte)?;
                Ok((
                    len + len2 + len3,
                    Data {
                        init: bytes,
                        mode: DataMode::Active(MemIdx(0), offset),
                    },
                ))
            }
            0x01 => {
                let (len2, bytes) = self.parse_vec(Self::parse_byte)?;
                Ok((
                    len + len2,
                    Data {
                        init: bytes,
                        mode: DataMode::Passive,
                    },
                ))
            }
            0x02 => {
                let (len2, memidx) = self.parse_u32()?;
                if memory_section.0.len() as u32 <= memidx {
                    Err(WasmParserError::InvalidMemIdx(0))?;
                }
                let (len3, offset) = self.parse_const_expr()?;

                let (len4, bytes) = self.parse_vec(Self::parse_byte)?;
                Ok((
                    len + len2 + len3 + len4,
                    Data {
                        init: bytes,
                        mode: DataMode::Active(MemIdx(memidx), offset),
                    },
                ))
            }
            unknown => Err(WasmParserError::InvalidDataKind(unknown)),
        }
    }
    fn parse_section_type(&mut self) -> Result<Option<WasmSectionType>> {
        let kind = self.reader.read_one()?;
        let kind = if let Some(kind) = kind {
            kind
        } else {
            return Ok(None);
        };
        trace!("{kind}");
        use WasmSectionType::*;
        Ok(Some(match kind {
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
        }))
    }

    fn parse_type_section(&mut self, size: u32) -> Result<TypeSection> {
        let (len, funcs) = self.parse_vec(&Self::parse_functype)?;
        if len != size as usize {
            Err(WasmParserError::InvalidSectionSize)?
        }
        Ok(TypeSection(funcs))
    }
    fn parse_import_section(&mut self, size: u32) -> Result<ImportSection> {
        let (len, imports) = self.parse_vec(&Self::parse_import)?;
        if len != size as usize {
            Err(WasmParserError::InvalidSectionSize)?
        }
        Ok(ImportSection(imports))
    }
    fn parse_function_section(&mut self, size: u32) -> Result<FunctionSection> {
        let (len, funcs) = self.parse_vec(&Self::parse_typeidx)?;
        if len != size as usize {
            Err(WasmParserError::InvalidSectionSize)?
        }
        Ok(FunctionSection(funcs))
    }
    fn parse_table_section(&mut self, size: u32) -> Result<TableSection> {
        let (len, tables) = self.parse_vec(&Self::parse_table)?;
        if len != size as usize {
            Err(WasmParserError::InvalidSectionSize)?
        }
        Ok(TableSection(tables))
    }
    fn parse_global_section(&mut self, size: u32) -> Result<GlobalSection> {
        let (len, globals) = self.parse_vec(&Self::parse_global)?;
        if len != size as usize {
            Err(WasmParserError::InvalidSectionSize)?
        }
        Ok(GlobalSection(globals))
    }
    fn parse_memory_section(&mut self, size: u32) -> Result<MemorySection> {
        let (len, memories) = self.parse_vec(&Self::parse_memtype)?;
        if memories.len() > 1 {
            Err(WasmParserError::MultipleMemory)?
        }
        if len != size as usize {
            Err(WasmParserError::InvalidSectionSize)?
        }
        Ok(MemorySection(memories))
    }
    fn parse_export_section(&mut self, size: u32) -> Result<ExportSection> {
        let (len, exports) = self.parse_vec(&Self::parse_export)?;
        if len != size as usize {
            Err(WasmParserError::InvalidSectionSize)?
        }
        Ok(ExportSection(exports))
    }
    fn parse_element_section(
        &mut self,
        function_section: &FunctionSection,
        size: u32,
    ) -> Result<ElementSection> {
        let (len, elems) = self.parse_vec(|me| me.parse_elem(function_section))?;
        if len != size as usize {
            Err(WasmParserError::InvalidSectionSize)?
        }
        Ok(ElementSection(elems))
    }
    fn parse_code_section(
        &mut self,
        type_section: &TypeSection,
        function_section: &FunctionSection,
        global_section: &GlobalSection,
        table_section: &TableSection,
        memory_section: &MemorySection,
        data_count_section: &mut DataCountVerifier,
        size: u32,
    ) -> Result<CodeSection> {
        let mut idx = 0;
        let (len, codes) = self.parse_vec(|me| {
            let r = Self::parse_code(
                me,
                type_section,
                function_section,
                global_section,
                table_section,
                memory_section,
                data_count_section,
                FuncIdx(idx),
            )?;
            idx += 1;
            Ok(r)
        })?;
        if len != size as usize {
            Err(WasmParserError::InvalidSectionSize)?
        }
        Ok(CodeSection(codes))
    }

    fn parse_data_section(
        &mut self,
        memory_section: &MemorySection,
        size: u32,
    ) -> Result<DataSection> {
        let (len, d) = self.parse_vec(|me| me.parse_data(memory_section))?;
        if len != size as usize {
            Err(WasmParserError::InvalidSectionSize)?
        }
        Ok(DataSection(d))
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
        let mut memory_section: Option<MemorySection> = None;
        let mut element_section: Option<ElementSection> = None;
        let mut table_section: Option<TableSection> = None;
        let mut code_section: Option<CodeSection> = None;
        let mut import_section: Option<ImportSection> = None;
        let mut data_section: Option<DataSection> = None;
        let mut data_count_verifier = DataCountVerifier::Lazy { max_data_idx: None };

        loop {
            let st = self.parse_section_type()?;
            let st = if let Some(st) = st {
                st
            } else {
                let data_section = data_section.unwrap_or_else(|| DataSection(vec![]));
                if let DataCountVerifier::Lazy {
                    max_data_idx: Some(max_data_idx),
                } = data_count_verifier
                {
                    if max_data_idx as usize >= data_section.0.len() {
                        Err(WasmParserError::InvalidDataSectionCount)?;
                    }
                }

                return Ok(Module {
                    fts: type_section.unwrap_or_else(|| TypeSection(vec![])),
                    xs: function_section.unwrap_or_else(|| FunctionSection(vec![])),
                    gs: global_section.unwrap_or_else(|| GlobalSection(vec![])),
                    tables: table_section.unwrap_or_else(|| TableSection(vec![])),
                    mems: memory_section.unwrap_or_else(|| MemorySection(vec![])),
                    elems: element_section.unwrap_or_else(|| ElementSection(vec![])),
                    exs: export_section.unwrap_or_else(|| ExportSection(vec![])),
                    codes: code_section.unwrap_or_else(|| CodeSection(vec![])),
                    data: data_section,
                });
            };
            match st {
                WasmSectionType::Custom => {
                    let (_, size) = self.parse_u32()?;
                    self.skip_section(size)?;
                }
                WasmSectionType::Type => {
                    trace!("type section");
                    type_section = Some(self.parse_section_body(Self::parse_type_section)?);
                }
                WasmSectionType::Import => {
                    import_section = Some(self.parse_section_body(Self::parse_import_section)?);
                }
                WasmSectionType::Function => {
                    function_section = Some(self.parse_section_body(Self::parse_function_section)?);
                }
                WasmSectionType::Table => {
                    table_section = Some(self.parse_section_body(Self::parse_table_section)?);
                }
                WasmSectionType::Memory => {
                    if memory_section.is_some() {
                        Err(WasmParserError::MultipleMemory)?;
                    }
                    let import_sec = import_section.get_or_insert_with(|| ImportSection(vec![]));
                    let section = self.parse_section_body(Self::parse_memory_section)?;
                    let imported_memory_count = import_sec
                        .0
                        .iter()
                        .filter(|v| matches!(v.desc, ImportDesc::MemType(_)))
                        .count();
                    let defined_memory_count = section.0.len();
                    if imported_memory_count + defined_memory_count > 1 {
                        Err(WasmParserError::MultipleMemory)?;
                    }
                    memory_section = Some(section);
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
                    trace!("element section");
                    let function_section =
                        function_section.get_or_insert_with(|| FunctionSection(vec![]));

                    element_section = Some(self.parse_section_body(|me, size| {
                        me.parse_element_section(function_section, size)
                    })?);
                }
                WasmSectionType::Code => {
                    let type_section = type_section.as_ref().unwrap();
                    let function_section = function_section.as_ref().unwrap();
                    let global_section =
                        global_section.get_or_insert_with(|| GlobalSection(vec![]));
                    let table_section = table_section.get_or_insert_with(|| TableSection(vec![]));
                    code_section = Some(self.parse_section_body(|me, size| {
                        Self::parse_code_section(
                            me,
                            &type_section,
                            &function_section,
                            &global_section,
                            &table_section,
                            &memory_section.get_or_insert_with(|| MemorySection(vec![])),
                            &mut data_count_verifier,
                            size,
                        )
                    })?);
                }
                WasmSectionType::Data => {
                    let sec = self.parse_section_body(|me, size| {
                        me.parse_data_section(
                            memory_section.get_or_insert_with(|| MemorySection(vec![])),
                            size,
                        )
                    })?;

                    data_section = Some(sec);
                }
                WasmSectionType::DataCount => {
                    let count = self.parse_section_body(|me, size| {
                        let (len, count) = me.parse_u32()?;
                        if size as usize != len {
                            Err(WasmParserError::InvalidSectionSize)?
                        }
                        Ok(count)
                    })?;
                    data_count_verifier = DataCountVerifier::OnePass(count);
                }
            }
        }
    }
}
