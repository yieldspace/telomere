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
#[derive(Debug, Clone)]
pub struct ResultType(pub Vec<ValType>);
impl ResultType {
    pub fn stack_pop_iter(&self) -> impl Iterator<Item = &ValType> + use<'_> {
        self.0.iter().rev()
    }
    pub fn iter(&self) -> impl Iterator<Item = &ValType> + use<'_> {
        self.0.iter()
    }
}

#[derive(Debug, Clone)]
pub struct FuncType(pub ResultType, pub ResultType);
#[derive(Debug, Clone)]
pub struct TypeSection(pub Vec<FuncType>);
impl TypeSection {
    pub fn get(&self, idx: TypeIdx) -> Option<&FuncType> {
        self.0.get(idx.0 as usize)
    }
}
#[derive(Debug, Clone)]

pub struct FunctionSection(pub Vec<TypeIdx>);
impl FunctionSection {
    pub fn get(&self, idx: FuncIdx) -> Option<TypeIdx> {
        self.0.get(idx.0 as usize).copied()
    }
}
#[derive(Debug, Clone)]
pub struct ExportSection(pub Vec<Export>);
impl ExportSection {
    pub fn find(&self, name: &str) -> Option<ExportDesc> {
        self.0.iter().find(|it| it.0 == name).map(|it| it.1)
    }
}
#[derive(Debug, Clone)]
pub struct GlobalSection(pub Vec<Global>);
impl GlobalSection {
    pub fn iter(&self) -> impl Iterator<Item = &Global> + use<'_> {
        self.0.iter()
    }
}
pub struct CodeSection(pub Vec<Func>);
impl CodeSection {
    pub fn get(&self, idx: FuncIdx) -> Option<&Func> {
        self.0.get(idx.0 as usize)
    }
}
pub struct MemType {
    pub min: u32,
    pub max: Option<u32>,
}
pub struct MemorySection(pub Vec<MemType>);
pub struct Module {
    pub fts: TypeSection,
    pub xs: FunctionSection,
    pub mems: MemorySection,
    pub gs: GlobalSection,
    pub exs: ExportSection,
    pub codes: CodeSection,
}
pub struct Instance {
    pub memory: Memory,
    pub globals: Vec<u8>,
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
pub struct Global(pub GlobalType, pub WasmValue);
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
pub struct Export(pub String, pub ExportDesc);
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
    pub drop_size: usize,
    pub local_addr: u32,
    pub select: usize,
    pub memarg: MemArg,
}
#[derive(Debug)]
pub enum VMError {
    Unreachable,
    StackOverflow,
    MemoryIndexOutOfRange,
}
pub type Op = unsafe fn(*const Instr, &mut ExecuteContext) -> Result<u32, VMError>;
pub union Instr {
    pub op: Op,
    pub operand: Operand,
}
unsafe impl Send for Instr {}
unsafe impl Sync for Instr {}
#[derive(Debug, Clone)]
pub enum WasmValue {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    //V128,
    //FuncRef,
    //ExternRef,
}
pub const PAGE_SIZE: usize = 64 * 1024;
pub const PAGE_SIZE_MAX: usize = 4 * 1024 * 1024 * 1024 / PAGE_SIZE;
pub struct Memory(pub Vec<u8>);
impl Memory {
    pub fn read_u8_array<const N: usize>(&self, offset: usize) -> Result<[u8; N], VMError> {
        let mut arr = [0u8; N];
        arr.copy_from_slice(
            &self
                .0
                .get(offset..offset + N)
                .ok_or_else(|| VMError::MemoryIndexOutOfRange)?,
        );
        Ok(arr)
    }
    fn write_slice(&mut self, memarg: MemArg, offset: u32, value: &[u8]) -> Result<(), VMError> {
        self.0
            .get_mut(
                (memarg.offset + offset) as usize..(memarg.offset + offset) as usize + value.len(),
            )
            .ok_or_else(|| VMError::MemoryIndexOutOfRange)?
            .copy_from_slice(value);
        Ok(())
    }
    pub fn write_u32(&mut self, memarg: MemArg, offset: u32, value: u32) -> Result<(), VMError> {
        self.write_slice(memarg, offset, &value.to_le_bytes())
    }
    pub fn write_u8(&mut self, memarg: MemArg, offset: u32, value: u8) -> Result<(), VMError> {
        *self
            .0
            .get_mut((memarg.offset + offset) as usize)
            .ok_or_else(|| VMError::MemoryIndexOutOfRange)? = value;
        Ok(())
    }
    pub fn write_u16(&mut self, memarg: MemArg, offset: u32, value: u16) -> Result<(), VMError> {
        self.write_slice(memarg, offset, &value.to_le_bytes())?;
        Ok(())
    }
    pub fn read_u32(&self, memarg: MemArg, offset: u32) -> Result<u32, VMError> {
        Ok(u32::from_le_bytes(
            self.read_u8_array::<4>((memarg.offset + offset) as usize)?,
        ))
    }
    pub fn read_u8(&self, memarg: MemArg, offset: u32) -> Result<u8, VMError> {
        Ok(self.read_u8_array::<1>((memarg.offset + offset) as usize)?[0])
    }
    pub fn read_i8(&self, memarg: MemArg, offset: u32) -> Result<i8, VMError> {
        Ok(self.read_u8_array::<1>((memarg.offset + offset) as usize)?[0] as i8)
    }
    pub fn page_size(&self) -> u32 {
        (self.0.len() / PAGE_SIZE) as u32
    }
    pub fn grow(&mut self, page_size_delta: u32) {
        self.0
            .resize((self.page_size() + page_size_delta) as usize * PAGE_SIZE, 0);
    }
}
pub struct ExecuteContext<'a> {
    pub module: &'a Module,
    pub stack: &'a mut Stack,

    pub instance: &'a mut Instance,
    pub local_state: Vec<LocalState<'a>>,
}
impl<'a> ExecuteContext<'a> {
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

pub struct JumpTable(Vec<u32>);
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
    pub fn end(&mut self) {
        self.0.pop();
    }
}
pub struct Stack {
    memory: Box<[u8]>,
    top: usize,
}
#[derive(Debug, Clone, Copy)]
pub struct LocalReference {
    local_top: usize,
    local_size: usize,
}
impl Stack {
    pub fn new(size: usize) -> Self {
        let mut vec = Vec::with_capacity(size);
        vec.resize(size, 0);
        Stack {
            memory: vec.into_boxed_slice(),
            top: 0,
        }
    }
    pub fn push_u8_array<const N: usize>(&mut self, v: [u8; N]) {
        unsafe {
            std::ptr::copy(
                v.as_ptr(),
                self.memory[self.top..self.top + N].as_mut_ptr(),
                N,
            )
        };
        self.top += N;
    }

    pub fn push_slice(&mut self, v: &[u8]) {
        unsafe {
            std::ptr::copy(
                v.as_ptr(),
                self.memory[self.top..self.top + v.len()].as_mut_ptr(),
                v.len(),
            )
        };
        self.top += v.len();
    }
    pub fn pop_u8_array<const N: usize>(&mut self) -> [u8; N] {
        self.top -= N;
        let mut arr = [0u8; N];
        unsafe {
            std::ptr::copy(
                self.memory[self.top..self.top + N].as_ptr(),
                arr.as_mut_ptr(),
                N,
            )
        };
        arr
    }
    pub fn pop_u8_array_generic<const N: usize>(&mut self, n: usize) -> [u8; N] {
        self.top -= n;
        let mut arr = [0u8; N];
        unsafe {
            std::ptr::copy(
                self.memory[self.top..self.top + n].as_ptr(),
                arr.as_mut_ptr(),
                N,
            )
        };
        arr
    }
    pub fn drop(&mut self, n: usize) -> &[u8] {
        self.top -= n;
        &self.memory[self.top..self.top + n]
    }
    pub fn push_u32(&mut self, v: u32) {
        self.push_u8_array(v.to_le_bytes());
    }
    pub fn pop_u32(&mut self) -> u32 {
        u32::from_le_bytes(self.pop_u8_array::<4>())
    }
    pub fn push_u64(&mut self, v: u64) {
        self.push_u8_array(v.to_le_bytes());
    }
    pub fn pop_u64(&mut self) -> u64 {
        u64::from_le_bytes(self.pop_u8_array::<8>())
    }
    pub fn push_i32(&mut self, v: i32) {
        self.push_u8_array(v.to_le_bytes());
    }
    pub fn push_f32(&mut self, v: f32) {
        self.push_u8_array(v.to_le_bytes());
    }
    pub fn push_f64(&mut self, v: f64) {
        self.push_u8_array(v.to_le_bytes());
    }
    pub fn pop_i32(&mut self) -> i32 {
        i32::from_le_bytes(self.pop_u8_array::<4>())
    }
    pub fn push_i64(&mut self, v: i64) {
        self.push_u8_array(v.to_le_bytes());
    }
    pub fn pop_i64(&mut self) -> i64 {
        i64::from_le_bytes(self.pop_u8_array::<8>())
    }
    pub fn pop_f32(&mut self) -> f32 {
        f32::from_le_bytes(self.pop_u8_array::<4>())
    }
    pub fn pop_f64(&mut self) -> f64 {
        f64::from_le_bytes(self.pop_u8_array::<8>())
    }
    pub fn access_locals(&mut self, reference: &LocalReference) -> &mut [u8] {
        &mut self.memory[reference.local_top..self.top + reference.local_size]
    }
    pub fn local_get(&mut self, reference: &LocalReference, local_addr: usize, size: usize) {
        self.memory.copy_within(
            reference.local_top + local_addr..reference.local_top + local_addr + size,
            self.top,
        );
        self.top += size;
    }
    pub fn local_set(&mut self, reference: &LocalReference, local_addr: usize, size: usize) {
        self.top -= size;
        self.memory
            .copy_within(self.top..self.top + size, reference.local_top + local_addr);
    }
    pub fn local_tee(&mut self, reference: &LocalReference, local_addr: usize, size: usize) {
        self.memory
            .copy_within(self.top - size..self.top, reference.local_top + local_addr);
    }
    pub fn function_call(
        &mut self,
        param_size: usize,
        local_size: usize,
        return_addr: *const Instr,
    ) -> LocalReference {
        let local_top = self.top - param_size;
        self.top += local_size;
        self.push_slice(&(return_addr as usize).to_le_bytes());
        LocalReference {
            local_top,
            local_size: param_size + local_size + std::mem::size_of_val(&return_addr),
        }
    }
    pub fn function_return(
        &mut self,
        reference: &LocalReference,
        return_size: usize,
    ) -> *const Instr {
        let return_addr_addr =
            reference.local_top + reference.local_size - std::mem::size_of::<usize>();
        let mut buf = [0; std::mem::size_of::<usize>()];
        buf.copy_from_slice(
            &self.memory[return_addr_addr..return_addr_addr + std::mem::size_of::<usize>()],
        );
        let return_addr = usize::from_le_bytes(buf);

        self.memory
            .copy_within(self.top - return_size..self.top, reference.local_top);
        self.top = reference.local_top + return_size;
        return_addr as *const Instr
    }
}
