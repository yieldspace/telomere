use thiserror::Error;

use crate::common::{FuncIdx, Limits, TypeIdx, ValType};

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
    #[error("unknown export")]
    UnknownExport,
    #[error("duplicated export")]
    DuplicatedExport(String),
    #[error("invalid result arity")]
    InvalidResultArity,
    #[error("start function")]
    StartFunction,
    #[error("size minimum must not be greater than maximum")]
    InvalidLimit,
    #[error("unknown element")]
    UnknownElement,
}
impl WasmParserError {
    pub fn invalid_instruction1(inst: u8) -> WasmParserError {
        WasmParserError::InvalidInstruction([inst, 0, 0, 0])
    }
}
