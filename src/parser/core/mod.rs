use crate::common::Instr;
use crate::runtime::vm::WasmValue;
mod dispatch;
mod parser;

#[derive(Debug, Clone, Copy)]
pub struct TypeIdx(u32);
#[derive(Debug, Clone, Copy)]
pub struct FuncIdx(u32);
#[derive(Debug, Clone, Copy)]
pub struct TableIdx(u32);
#[derive(Debug, Clone, Copy)]
pub struct MemIdx(u32);
#[derive(Debug, Clone, Copy)]
pub struct GlobalIdx(u32);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValType {
    I32,
    I64,
    F32,
    F64,
    V128,
    FuncRef,
    ExternRef,
}
#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum ValueSize {
    Byte4,
    Byte8,
    Byte16,
}
impl ValueSize {
    pub fn u32(&self) -> u32 {
        match self {
            Self::Byte4 => 4,
            Self::Byte8 => 8,
            Self::Byte16 => 16,
        }
    }
    pub fn usize(&self) -> usize {
        match self {
            Self::Byte4 => 4,
            Self::Byte8 => 8,
            Self::Byte16 => 16,
        }
    }
}

impl ValType {
    pub fn stack_size(&self) -> ValueSize {
        match self {
            ValType::ExternRef => ValueSize::Byte8,
            ValType::F32 => ValueSize::Byte4,
            ValType::F64 => ValueSize::Byte8,
            ValType::FuncRef => ValueSize::Byte8,
            ValType::I32 => ValueSize::Byte4,
            ValType::I64 => ValueSize::Byte8,
            ValType::V128 => ValueSize::Byte16,
        }
    }
}
#[derive(Debug, Clone)]
pub struct ResultType(pub Vec<ValType>);
impl ResultType {
    pub fn iter(&self) -> impl Iterator<Item = &ValType> + use<'_> {
        self.0.iter().rev()
    }
    pub fn rev_iter(&self) -> impl Iterator<Item = &ValType> + use<'_> {
        self.0.iter()
    }
}

#[derive(Debug, Clone)]
pub struct FuncType(pub ResultType, pub ResultType);
#[derive(Debug, Clone)]
pub struct TypeSection(Vec<FuncType>);
impl TypeSection {
    pub fn get(&self, idx: TypeIdx) -> Option<&FuncType> {
        self.0.get(idx.0 as usize)
    }
}
#[derive(Debug, Clone)]

pub struct FunctionSection(Vec<TypeIdx>);
impl FunctionSection {
    pub fn get(&self, idx: FuncIdx) -> Option<TypeIdx> {
        self.0.get(idx.0 as usize).copied()
    }
}
#[derive(Debug, Clone)]
pub struct ExportSection(Vec<Export>);
impl ExportSection {
    pub fn find(&self, name: &str) -> Option<ExportDesc> {
        self.0.iter().find(|it| it.0 == name).map(|it| it.1)
    }
}
#[derive(Debug, Clone)]
pub struct GlobalSection(Vec<Global>);
impl GlobalSection {
    pub fn iter(&self) -> impl Iterator<Item = &Global> + use<'_> {
        self.0.iter()
    }
}
pub struct CodeSection(Vec<Func>);
impl CodeSection {
    pub fn get(&self, idx: FuncIdx) -> Option<&Func> {
        self.0.get(idx.0 as usize)
    }
}
pub struct Module {
    pub fts: TypeSection,
    pub xs: FunctionSection,
    pub gs: GlobalSection,
    pub exs: ExportSection,
    pub codes: CodeSection,
}
#[derive(Debug, Clone)]
pub struct Locals {
    pub n: u32,
    pub t: ValType,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Mut {
    Const = 0,
    Var = 1,
}
#[derive(Debug, Clone, Copy)]
pub struct GlobalType(pub ValType, pub Mut);
#[derive(Debug, Clone)]
pub struct Global(pub GlobalType,pub WasmValue);
#[derive()]
pub struct Func {
    pub locals: Vec<Locals>,
    pub expr: Vec<Instr>,
}
#[derive(Debug, Clone, Copy)]
pub enum ExportDesc {
    Func(FuncIdx),
    Table(TableIdx),
    Mem(MemIdx),
    Global(GlobalIdx),
}
#[derive(Debug, Clone)]
pub struct Export(String, ExportDesc);
#[derive(Debug, Clone, Copy)]
#[repr(u64)]
pub enum BlockType {
    Void,
    ValType(ValType),
    TypeIdx(TypeIdx),
}
#[derive(Debug, Clone, Copy)]
pub struct MemArg {
    pub align: u32,
    pub offset: u32,
}

pub use parser::WasmParser;
pub use parser::WasmParserError;
