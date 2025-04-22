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
    #[error("constant expression required")]
    InvalidConstInstruction(u8),
    #[error("invalid blocktype")]
    InvalidBlockType(i64),
    #[error("type mismatch")]
    InvalidStackValType(ValType, Option<ValType>),
    #[error("type mismatch")]
    InvalidStackValTypeAny,
    #[error("unknown function {0}")]
    InvalidFuncIdx(FuncIdx),
    #[error("unknown type")]
    InvalidTypeIdx(TypeIdx),
    #[error("unknown local")]
    InvalidLocalIndex(u32),
    #[error("invalid globalidx: {0:?}")]
    InvalidGlobalIndex(u32),
    #[error("invalid mut: {0:?}")]
    InvalidMut(u8),
    #[error("unknown global")]
    UnknownGlobal,
    #[error("global is immutable")]
    InvalidGlobalAccess,
    #[error("invalid elem kind: {0}")]
    InvalidElemKind(u8),
    #[error("type mismatch")]
    InvalidElementSectionType(u32),
    #[error("unknown table")]
    InvalidTableIndex(u32),
    #[error("invalid table type: {0}")]
    InvalidTableType(u32),
    #[error("multiple memories")]
    MultipleMemory,
    #[error("invalid import desc: {0}")]
    InvalidImportDesc(u8),
    #[error("invalid data kind: {0}")]
    InvalidDataKind(u32),
    #[error("unknown memory {0}")]
    InvalidMemIdx(u32),
    #[error("memory size must be at most 65536 pages (4GiB)")]
    InvalidMemorySize(Limits),
    #[error("invalid alignment: {0}")]
    InvalidAlignment(u32),
    #[error("unknown data segment {0}")]
    InvalidDataIdx(u32),
    #[error("invalid data section count")]
    InvalidDataSectionCount,
    #[error("unknown function")]
    UnknownExport,
    #[error("duplicate export name")]
    DuplicatedExport(String),
    #[error("invalid result arity")]
    InvalidResultArity,
    #[error("start function")]
    StartFunction,
    #[error("size minimum must not be greater than maximum")]
    InvalidLimit,
    #[error("unknown elem segment {0}")]
    UnknownElement(u32),
    #[error("invalid section type: {0}")]
    InvalidSectionType(u8),
    #[error("too many locals")]
    TooManyLocals,
    #[error("function and code section have inconsistent lengths")]
    FunctionAndCodeSectionLengthMismatch,
    #[error("invalid section order")]
    InvalidSectionOrder,
    #[error("undeclared function reference")]
    UndeclaredFunctionReference,
    #[error("unknown label")]
    UnknownLabel
}
impl WasmParserError {
    pub fn invalid_instruction1(inst: u8) -> WasmParserError {
        WasmParserError::InvalidInstruction([inst, 0, 0, 0])
    }
}
