use std::{cell::RefCell, fmt::Debug, rc::Rc};
#[macro_use]
mod vm_result;
pub use vm_result::VMResult;
mod memory;
pub use memory::{MemArg, Memory};
mod stack;
pub use stack::{LocalReference, Stack};
mod registry;
pub use registry::Registry;
mod store;
pub use store::Store;
#[derive(Debug, Clone, Copy)]
pub struct TypeIdx(pub u32);
#[derive(Debug, Clone, Copy)]
pub struct FuncIdx(pub u32);
#[derive(Debug, Clone, Copy)]
pub struct TableIdx(pub u32);
#[derive(Debug, Clone, Copy)]
pub struct MemIdx(pub u32);
#[derive(Debug, Clone, Copy)]
pub struct GlobalIdx(pub u32);
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultType(pub Vec<ValType>);
impl ResultType {
    pub fn stack_pop_iter(&self) -> impl Iterator<Item = &ValType> + use<'_> {
        self.0.iter().rev()
    }
    pub fn iter(&self) -> impl Iterator<Item = &ValType> + use<'_> {
        self.0.iter()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableType {
    pub reftype: RefType,
    pub limits: Limits,
}
#[derive(Debug)]
pub struct Table(pub TableType);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuncType(pub ResultType, pub ResultType);
#[derive(Debug, Clone)]
pub struct TypeSection(pub Vec<FuncType>);
impl TypeSection {
    pub fn get(&self, idx: TypeIdx) -> Option<&FuncType> {
        self.0.get(idx.0 as usize)
    }
}
#[derive(Debug, Clone)]

pub enum ImportDesc {
    TypeIdx(TypeIdx),
    TableType(TableType),
    MemType(MemType),
    GlobalType(GlobalType),
}

#[derive(Debug, Clone)]
pub struct Import {
    pub module: String,
    pub name: String,
    pub desc: ImportDesc,
}
#[derive(Debug, Clone)]
pub struct ImportSection(pub Vec<Import>);

#[derive(Debug, Clone)]
pub struct FunctionSection(pub Vec<TypeIdx>);

#[derive(Debug, Clone)]
pub struct ExportSection(pub Vec<Export>);
impl ExportSection {
    pub fn find(&self, name: &str) -> Option<ExportDesc> {
        self.0.iter().find(|it| it.0 == name).map(|it| it.1)
    }
}
#[derive(Clone)]
pub struct CodeSection(pub Vec<Func>);
impl CodeSection {
    pub fn get(&self, idx: FuncIdx) -> Option<&Func> {
        self.0.get(idx.0 as usize)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub min: u32,
    pub max: Option<u32>,
}
#[derive(Debug, Clone, Copy)]
pub struct MemType(pub Limits);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefType {
    FuncRef,
    ExternRef,
}
#[derive(Debug, Clone, Copy)]
pub enum ElemMode {
    Passive,
    Active(TableIdx, WasmValue),
    Declarative,
}
#[derive(Debug, Clone)]
pub struct Elem {
    pub kind: RefType,
    pub init: Vec<u32>,
    pub mode: ElemMode,
}
#[derive(Debug, Clone)]
pub struct ElementSection(pub Vec<Elem>);
#[derive(Debug, Clone, Copy)]
pub enum DataMode {
    Passive,
    Active(MemIdx, WasmValue),
}
#[derive(Debug, Clone)]
pub struct Data {
    pub init: Vec<u8>,
    pub mode: DataMode,
}
pub enum DataCountVerifier {
    OnePass(u32),
    Lazy { max_data_idx: Option<u32> },
}

#[derive(Debug, Clone)]
pub struct DataSection(pub Vec<Data>);
#[derive(Clone)]
pub struct Module {
    pub fts: TypeSection,
    pub functions: Vec<TypeIdx>,
    pub imports: ImportSection,
    pub mems: Vec<MemType>,
    pub globals: Vec<GlobalType>,
    pub global_init: Vec<WasmValue>,
    pub exs: ExportSection,
    pub tables: Vec<TableType>,
    pub elems: ElementSection,
    pub codes: CodeSection,
    pub data: DataSection,
}
#[derive(Debug, Clone)]
pub struct TableInstance(pub TableType, pub Vec<u32>);
#[derive(Clone)]
pub struct Instance {
    pub memory: Option<Rc<RefCell<Memory>>>,
    pub table: Vec<TableInstance>,
    pub globals: Vec<u32>,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalType(pub ValType, pub Mut);
#[derive(Debug, Clone)]
pub struct Global(pub GlobalType, pub Vec<WasmValue>);
#[derive(Clone)]
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
pub struct Export(pub String, pub ExportDesc);
#[derive(Debug, Clone, Copy)]
#[repr(u64)]
pub enum BlockType {
    Void,
    ValType(ValType),
    TypeIdx(TypeIdx),
}

#[derive(Debug, Clone, Copy)]
pub struct LoopParam {
    pub stack_top: u32,
    pub param_size: u32,
}
#[derive(Debug, Clone, Copy)]
pub struct BlockReturn {
    pub stack_top: u32,
    pub return_size: u32,
}
#[derive(Clone, Copy)]
pub union Operand {
    pub i32: i32,
    pub u32: u32,
    pub i64: i64,
    pub u64: u64,
    pub f32: f32,
    pub f64: f64,

    pub jump_addr: u32,
    pub jump_addr2: (u32, u32),
    pub drop_size: u32,
    pub local_addr: u32,
    pub select: u32,
    pub memarg: MemArg,
    pub block_return: BlockReturn,
    pub loop_param: LoopParam,
}

pub type Op = unsafe fn(*const Instr, &mut ExecuteContext) -> VMResult<()>;
#[derive(Clone, Copy)]
pub union Instr {
    pub op: Op,
    pub operand: Operand,
}
unsafe impl Send for Instr {}
unsafe impl Sync for Instr {}
#[derive(Debug, Clone, Copy)]
pub enum WasmValue {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    //V128,
    FuncRef(u32),
    //ExternRef,
}
pub const PAGE_SIZE: usize = 64 * 1024;
pub const PAGE_SIZE_MAX: usize = 4 * 1024 * 1024 * 1024 / PAGE_SIZE;

pub struct ExecuteContext<'a> {
    pub module: &'a Module,
    pub stack: &'a mut Stack,
    pub local_state: Vec<LocalState<'a>>,
    pub table: &'a mut [TableInstance],
    pub globals: &'a mut [u32],
    pub memory: &'a mut Memory,
    pub store: &'a mut Store,
}
impl ExecuteContext<'_> {
    pub fn jump_table(&mut self) -> &mut JumpTable {
        unsafe { &mut self.local_state.last_mut().unwrap_unchecked().jump_table }
    }
    pub fn code(&self) -> *const Instr {
        unsafe { self.local_state.last().unwrap_unchecked().code.as_ptr() }
    }
    pub fn local_reference(&self) -> LocalReference {
        unsafe { self.local_state.last().unwrap_unchecked().local_reference }
    }
}
pub struct LocalState<'a> {
    // TODO: We should resolve jump address during instantiate time
    pub jump_table: JumpTable,
    // TODO: We should write this to stack and holds current only.
    pub local_reference: LocalReference,
    // TODO: We should write this to stack and holds current code or may avoid this?
    pub code: &'a [Instr],
}
#[derive(Debug)]
pub struct JumpTable(Vec<u32>);
impl Default for JumpTable {
    fn default() -> Self {
        Self::new()
    }
}

impl JumpTable {
    pub fn new() -> Self {
        Self(Vec::new())
    }
    pub fn push(&mut self, addr: u32) {
        self.0.push(addr);
    }
    pub fn br(&mut self, idx: usize) -> Option<u32> {
        self.0.drain(self.0.len() - 1 - idx..).next()
    }
    pub fn ret(&mut self) -> u32 {
        unsafe { self.0.drain(0..).next().unwrap_unchecked() }
    }
    pub fn end(&mut self) {
        self.0.pop();
    }
}
