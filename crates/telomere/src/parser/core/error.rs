use std::fmt;

use thiserror::Error;

use crate::common::{FuncIdx, Limits, TypeIdx, ValType};

/// A WebAssembly proposal identified while parsing an unsupported encoding.
///
/// The parser uses this type both for Cargo-feature gates and for proposals
/// that telomere does not implement yet, so embedders can distinguish an
/// unsupported proposal from malformed binary input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposalFeature {
    /// The fixed-width SIMD proposal.
    Simd,
    /// The threads and atomics proposal.
    Threads,
    /// The relaxed SIMD proposal.
    RelaxedSimd,
    /// The WebAssembly garbage-collection proposal.
    GarbageCollection,
    /// The exception-handling proposal.
    ExceptionHandling,
    /// The memory64 proposal.
    Memory64,
    /// The custom-page-sizes proposal.
    CustomPageSizes,
    /// The wide-arithmetic proposal.
    WideArithmetic,
    /// The extended-const proposal.
    ExtendedConst,
}

impl fmt::Display for ProposalFeature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Simd => f.write_str("simd"),
            Self::Threads => f.write_str("threads"),
            Self::RelaxedSimd => f.write_str("relaxed-simd"),
            Self::GarbageCollection => f.write_str("gc"),
            Self::ExceptionHandling => f.write_str("exception-handling"),
            Self::Memory64 => f.write_str("memory64"),
            Self::CustomPageSizes => f.write_str("custom-page-sizes"),
            Self::WideArithmetic => f.write_str("wide-arithmetic"),
            Self::ExtendedConst => f.write_str("extended-const"),
        }
    }
}

/// Errors produced while decoding or validating a core WebAssembly module.
///
/// An [`UnsupportedFeature`](Self::UnsupportedFeature) reports a recognised
/// proposal encoding, whereas the `Invalid*` variants describe malformed or
/// otherwise unsupported core-binary input.
#[derive(Error, Debug)]
pub enum WasmParserError {
    /// The module did not start with the WebAssembly magic bytes.
    #[error("invalid magic: {0:?}")]
    InvalidMagic([u8; 4]),
    /// The module declares an unsupported WebAssembly binary version.
    #[error("invalid version: {0:?}")]
    InvalidVersion([u8; 4]),
    /// A LEB128 integer was not encoded canonically or overflowed its width.
    #[error("invalid leb128 encoding")]
    InvalidLeb128Encoding,
    /// A section body did not consume exactly its declared size.
    #[error("invalid section size")]
    InvalidSectionSize,
    /// A type-section entry did not begin with the function-type marker.
    #[error("invalid function type signature: {0}")]
    InvalidFunctionTypeSignature(u8),
    /// A value-type byte is not valid in the parsed context.
    #[error("invalid value type: {0}")]
    InvalidValueType(u8),
    /// A length-prefixed UTF-8 name could not be decoded.
    #[error("invalid name encoding")]
    InvalidNameEncoding,
    /// An export descriptor uses an unknown kind byte.
    #[error("invalid export desc: {0}")]
    InvalidExportDesc(u8),
    /// A function body consumed a different number of bytes than declared.
    #[error("invalid instruction size: expected: {0:?}, actual: {1:?}")]
    InvalidInstructionSize(u32, u32),
    /// Reading from the module source failed.
    #[error("error from underlying layer")]
    IoError(#[from] std::io::Error),
    /// An opcode or prefixed subopcode has no recognised core instruction.
    #[error("invalid instruction: {0:?}")]
    InvalidInstruction([u8; 4]),
    /// A recognised proposal encoding is unavailable in this build.
    #[error("unsupported proposal feature '{feature}' for opcode {opcode:?}")]
    UnsupportedFeature {
        /// The proposal required by the rejected encoding.
        feature: ProposalFeature,
        /// A compact opcode diagnostic, beginning with the instruction prefix when present.
        opcode: [u8; 4],
    },
    /// An instruction is not permitted in a constant expression.
    #[error("constant expression required")]
    InvalidConstInstruction(u8),
    /// A block type is neither a valid value type nor a valid type index.
    #[error("invalid blocktype")]
    InvalidBlockType(i64),
    /// A value on the validation stack has the wrong type.
    #[error("type mismatch")]
    InvalidStackValType(ValType, Option<ValType>),
    /// A stack operation could not be validated against its required types.
    #[error("type mismatch")]
    InvalidStackValTypeAny,
    /// A function index does not name a declared function.
    #[error("unknown function {0}")]
    InvalidFuncIdx(FuncIdx),
    /// A type index does not name a declared function type.
    #[error("unknown type")]
    InvalidTypeIdx(TypeIdx),
    /// A local index is outside the function's parameter and local space.
    #[error("unknown local")]
    InvalidLocalIndex(u32),
    /// A global index is outside the module's global space.
    #[error("invalid globalidx: {0:?}")]
    InvalidGlobalIndex(u32),
    /// A global mutability byte is invalid.
    #[error("invalid mut: {0:?}")]
    InvalidMut(u8),
    /// A constant expression references an unknown global.
    #[error("unknown global")]
    UnknownGlobal,
    /// A constant expression attempts to read a mutable global.
    #[error("global is immutable")]
    InvalidGlobalAccess,
    /// An element segment uses an unknown element kind.
    #[error("invalid elem kind: {0}")]
    InvalidElemKind(u8),
    /// An element segment's declared reference type is incompatible with its contents.
    #[error("type mismatch")]
    InvalidElementSectionType(u32),
    /// A table index is outside the module's table space.
    #[error("unknown table")]
    InvalidTableIndex(u32),
    /// A table instruction refers to an invalid table type.
    #[error("invalid table type: {0}")]
    InvalidTableType(u32),
    /// A module declares more memories than this runtime accepts.
    #[error("multiple memories")]
    MultipleMemory,
    /// An import descriptor uses an unknown kind byte.
    #[error("invalid import desc: {0}")]
    InvalidImportDesc(u8),
    /// A data segment uses an unknown mode.
    #[error("invalid data kind: {0}")]
    InvalidDataKind(u32),
    /// A memory index is outside the module's memory space.
    #[error("unknown memory {0}")]
    InvalidMemIdx(u32),
    /// A memory's page count exceeds the currently supported 32-bit limit.
    #[error("memory size must be at most 65536 pages (4GiB)")]
    InvalidMemorySize(Limits),
    /// A memory argument's alignment exponent is invalid for its instruction.
    #[error("invalid alignment: {0}")]
    InvalidAlignment(u32),
    /// A data-segment index is outside the module's data space.
    #[error("unknown data segment {0}")]
    InvalidDataIdx(u32),
    /// A data-count section is missing or inconsistent with bulk-memory use.
    #[error("invalid data section count")]
    InvalidDataSectionCount,
    /// An export references a function, table, memory, or global that is absent.
    #[error("unknown function")]
    UnknownExport,
    /// More than one export uses the same name.
    #[error("duplicate export name")]
    DuplicatedExport(String),
    /// An instruction result arity cannot be represented by this runtime.
    #[error("invalid result arity")]
    InvalidResultArity,
    /// The start function does not have the required `() -> ()` signature.
    #[error("start function")]
    StartFunction,
    /// A limit has an invalid flag or a minimum greater than its maximum.
    #[error("size minimum must not be greater than maximum")]
    InvalidLimit,
    /// An element-segment index is outside the module's element space.
    #[error("unknown elem segment {0}")]
    UnknownElement(u32),
    /// A section ID is not part of the supported core binary format.
    #[error("invalid section type: {0}")]
    InvalidSectionType(u8),
    /// A function declares more locals than the parser can represent.
    #[error("too many locals")]
    TooManyLocals,
    /// Nested structured control exceeds the parser's explicit depth limit.
    #[error("nesting depth exceeds the limit of {limit}")]
    NestingTooDeep {
        /// The fixed maximum nesting depth accepted by the parser.
        limit: u32,
    },
    /// The function and code sections declare different function counts.
    #[error("function and code section have inconsistent lengths")]
    FunctionAndCodeSectionLengthMismatch,
    /// Sections appear in an order disallowed by the core binary format.
    #[error("invalid section order")]
    InvalidSectionOrder,
    /// A `ref.func` instruction targets a function that was not declared for references.
    #[error("undeclared function reference")]
    UndeclaredFunctionReference,
    /// A branch references a label that is not active in the current function.
    #[error("unknown label")]
    UnknownLabel,
}
impl WasmParserError {
    /// Creates an invalid-instruction diagnostic for a one-byte opcode.
    pub fn invalid_instruction1(inst: u8) -> WasmParserError {
        WasmParserError::InvalidInstruction([inst, 0, 0, 0])
    }

    /// Creates an error that identifies the unsupported proposal behind an opcode.
    pub fn unsupported_feature(feature: ProposalFeature, opcode: [u8; 4]) -> WasmParserError {
        WasmParserError::UnsupportedFeature { feature, opcode }
    }
}
