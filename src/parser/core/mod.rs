use crate::common::Instr;
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
#[derive(Debug, Clone, Copy)]
pub enum ValType {
    I32,
    I64,
    F32,
    F64,
    V128,
    FuncRef,
    ExternRef,
}
#[derive(Debug, Clone)]
pub struct ResultType(pub Vec<ValType>);
impl ResultType {
    pub fn iter(&self) -> impl Iterator<Item = &ValType> + use<'_> {
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
pub struct CodeSection(Vec<Func>);
impl CodeSection {
    pub fn get(&self, idx: FuncIdx) -> Option<&Func> {
        self.0.get(idx.0 as usize)
    }
}
#[derive(Debug, Clone)]
pub struct Module {
    pub fts: TypeSection,
    pub xs: FunctionSection,
    pub exs: ExportSection,
    pub codes: CodeSection,
}
#[derive(Debug, Clone)]
pub struct Locals {
    n: u32,
    t: ValType,
}
#[derive(Debug, Clone)]
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
pub use parser::WasmParser;
pub use parser::WasmParserError;
