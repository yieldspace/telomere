use std::mem::transmute;

use thiserror::Error;

use crate::{binary::BinaryReader, common::OPERAND_NONE};

use super::{
    CodeSection, Export, ExportDesc, ExportSection, Func, FuncIdx, FuncType, FunctionSection,
    GlobalIdx, Instr, Locals, MemIdx, Module, ResultType, TableIdx, TypeIdx, TypeSection, ValType,
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
    #[error("invalid instruction size")]
    InvalidInstructionSize,
    #[error("error from underlying layer")]
    IoError(#[from] std::io::Error),
    #[error("invalid instruction: {0:x}")]
    InvalidInstruction(u8),
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
    ($name: ident, $t: ident, $bit_size: expr, $is_signed: expr) => {
        fn $name(&mut self) -> Result<(usize, $t)> {
            let bit_size = $bit_size;
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
                result |= (!0 as $t) << (read_bytes * 7);
            }

            Ok((read_bytes, result))
        }
    };
    ($name: ident, $t: ident, $is_signed: expr) => {
        parse_leb128impl!($name, $t, std::mem::size_of::<$t>() * 8, $is_signed);
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
impl<'a, R: BinaryReader> WasmParser<'a, R> {
    fn parse_u32(&mut self) -> Result<(usize, u32)> {
        Leb128Parser::new(self.reader).parse_u32()
    }
    fn parse_i32(&mut self) -> Result<(usize, i32)> {
        Leb128Parser::new(self.reader).parse_i32()
    }
    fn parse_i64(&mut self) -> Result<(usize, i64)> {
        Leb128Parser::new(self.reader).parse_i64()
    }

    fn parse_vec<V>(
        &mut self,
        mut f: impl FnMut(&mut Self) -> Result<(usize, V)>,
    ) -> Result<(usize, Vec<V>)> {
        let mut read_bytes = 0;

        let (len_len, len) = self.parse_u32()?;
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
        read_bytes += 1;
        if signature != 0x60 {
            Err(WasmParserError::InvalidFunctionTypeSignature(signature))?
        }
        let (len, input) = self.parse_result_type()?;
        read_bytes += len;
        let (len, output) = self.parse_result_type()?;
        read_bytes += len;
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
    fn parse_inst(&mut self) -> Result<(usize, bool, Instr)> {
        let v = self.reader.read_exact_one()?;
        use crate::runtime::vm;
        Ok(match v {
            0x0F => (
                1,
                false,
                Instr {
                    op: vm::op_return,
                    operand: OPERAND_NONE,
                },
            ),
            0x0B => (
                1,
                true,
                Instr {
                    op: vm::op_end,
                    operand: OPERAND_NONE,
                },
            ),
            0x41 => {
                let (len, operand) = self.parse_i32()?;
                (
                    1 + len,
                    false,
                    Instr {
                        op: vm::op_i32_const,
                        operand: crate::common::Operand { i32: operand },
                    },
                )
            }
            0x42 => {
                let (len, operand) = self.parse_i64()?;
                (
                    1 + len,
                    false,
                    Instr {
                        op: vm::op_i64_const,
                        operand: crate::common::Operand { i64: operand },
                    },
                )
            }
            0x6A => (
                1,
                false,
                Instr {
                    op: vm::op_i32_add,
                    operand: OPERAND_NONE,
                },
            ),
            0x7C => (
                1,
                false,
                Instr {
                    op: vm::op_i64_add,
                    operand: OPERAND_NONE,
                },
            ),
            unknown => Err(WasmParserError::InvalidInstruction(unknown))?,
        })
    }
    fn parse_instrs(&mut self) -> Result<(usize, Vec<Instr>)> {
        let mut read_bytes = 0;
        let mut result = Vec::new();
        loop {
            let (len, end, instr) = self.parse_inst()?;
            read_bytes += len;
            result.push(instr);
            if end {
                return Ok((read_bytes, result));
            }
        }
    }
    fn parse_code_inner(&mut self, size: u32) -> Result<Func> {
        let (len, locals) = self.parse_vec(&Self::parse_locals)?;
        let (len2, instrs) = self.parse_instrs()?;
        if len + len2 != size as usize {
            Err(WasmParserError::InvalidInstructionSize)?
        }
        Ok(Func {
            locals,
            expr: instrs,
        })
    }
    fn parse_code(&mut self) -> Result<(usize, Func)> {
        let (len, size) = self.parse_u32()?;
        let func = self.parse_code_inner(size)?;
        Ok((len + size as usize, func))
    }

    fn parse_section_type(&mut self) -> Result<WasmSectionType> {
        let kind = self.reader.read_exact_one()?;
        if kind <= 12 {
            Ok(unsafe { transmute(kind) })
        } else {
            Ok(WasmSectionType::Custom)
        }
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

    fn parse_export_section(&mut self, size: u32) -> Result<ExportSection> {
        let (len, exports) = self.parse_vec(&Self::parse_export)?;
        if len != size as usize {
            Err(WasmParserError::InvalidSectionSize)?
        }
        Ok(ExportSection(exports))
    }

    fn parse_code_section(&mut self, size: u32) -> Result<CodeSection> {
        let (len, codes) = self.parse_vec(&Self::parse_code)?;
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
    fn skip_to_next_section(&mut self, ty: WasmSectionType) -> Result<()> {
        loop {
            if self.parse_section_type()? == ty {
                return Ok(());
            }
            let (_, size) = self.parse_u32()?;
            self.skip_section(size)?;
        }
    }
    pub fn new(reader: &'a mut R) -> Self {
        Self { reader }
    }
    pub fn parse_module(&mut self) -> Result<Module> {
        self.parse_magic()?;
        self.parse_version()?;
        self.skip_to_next_section(WasmSectionType::Type)?;
        let type_section = self.parse_section_body(Self::parse_type_section)?;
        self.skip_to_next_section(WasmSectionType::Function)?;
        let function_section = self.parse_section_body(Self::parse_function_section)?;
        self.skip_to_next_section(WasmSectionType::Export)?;
        let export_section = self.parse_section_body(Self::parse_export_section)?;
        self.skip_to_next_section(WasmSectionType::Code)?;
        let code_section = self.parse_section_body(Self::parse_code_section)?;
        Ok(Module {
            fts: type_section,
            xs: function_section,
            exs: export_section,
            codes: code_section,
        })
    }
}

