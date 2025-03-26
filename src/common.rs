use std::fmt::Debug;

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
#[derive(Debug, Clone, Copy)]
pub struct TableType {
    pub reftype: RefType,
    pub limits: Limits,
}
#[derive(Debug)]
pub struct Table(pub TableType);
#[derive(Debug)]
pub struct TableSection(pub Vec<Table>);

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
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub min: u32,
    pub max: Option<u32>,
}
#[derive(Debug, Clone)]
pub struct MemType(pub Limits);
pub struct MemorySection(pub Vec<MemType>);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefType {
    FuncRef,
    ExternRef,
}
#[derive(Debug)]
pub enum ElemMode {
    Passive,
    Active(TableIdx, WasmValue),
    Declarative,
}
#[derive(Debug)]
pub struct Elem {
    pub kind: RefType,
    pub init: Vec<u32>,
    pub mode: ElemMode,
}
#[derive(Debug)]
pub struct ElementSection(pub Vec<Elem>);
#[derive(Debug)]
pub enum DataMode {
    Passive,
    Active(MemIdx, WasmValue),
}
#[derive(Debug)]
pub struct Data {
    pub init: Vec<u8>,
    pub mode: DataMode,
}
pub enum DataCountVerifier {
    OnePass(u32),
    Lazy { max_data_idx: Option<u32> },
}

#[derive(Debug)]
pub struct DataSection(pub Vec<Data>);

pub struct Module {
    pub fts: TypeSection,
    pub xs: FunctionSection,
    pub mems: MemorySection,
    pub gs: GlobalSection,
    pub exs: ExportSection,
    pub tables: TableSection,
    pub elems: ElementSection,
    pub codes: CodeSection,
    pub data: DataSection,
}
pub struct TableInstance(pub TableType, pub Vec<u32>);
pub struct Instance {
    pub memory: Memory,
    pub table: Vec<TableInstance>,
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
#[derive(Debug)]
#[must_use]
pub enum VMResult<V> {
    Success(V),
    Unreachable,
    StackOverflow,
    MemoryIndexOutOfRange,
    TableIndexOutOfRange,
    CallIndirectInvalidType,
    TableUninitialized,
}

macro_rules! vm_try {
    ($expr: expr) => {
        match $expr {
            VMResult::Success(v) => v,
            VMResult::Unreachable => return VMResult::Unreachable,
            VMResult::StackOverflow => return VMResult::StackOverflow,
            VMResult::MemoryIndexOutOfRange => return VMResult::MemoryIndexOutOfRange,
            VMResult::TableIndexOutOfRange => return VMResult::TableIndexOutOfRange,
            VMResult::CallIndirectInvalidType => return VMResult::CallIndirectInvalidType,
            VMResult::TableUninitialized => return VMResult::TableUninitialized,
        }
    };
}
impl<V> VMResult<V> {
    pub fn from_option(opt: Option<V>, err: impl FnOnce() -> VMResult<V>) -> VMResult<V> {
        match opt {
            Some(v) => VMResult::Success(v),
            None => err(),
        }
    }
    pub fn unwrap(self) -> V {
        if let VMResult::Success(v) = self {
            return v;
        }
        panic!()
    }
    pub fn is_err(&self) -> bool {
        !matches!(self, VMResult::Success(_))
    }
}
pub type Op = unsafe fn(*const Instr, &mut ExecuteContext) -> VMResult<()>;
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
    FuncRef(u32),
    //ExternRef,
}
pub const PAGE_SIZE: usize = 64 * 1024;
pub const PAGE_SIZE_MAX: usize = 4 * 1024 * 1024 * 1024 / PAGE_SIZE;
pub struct Memory(pub Vec<u8>);
fn compute_offset(memarg: MemArg, offset: u32) -> VMResult<usize> {
    VMResult::from_option(
        memarg.offset.checked_add(offset).map(|v| v as usize),
        || VMResult::MemoryIndexOutOfRange,
    )
}
impl Memory {
    pub fn read_u8_array<const N: usize>(&self, offset: usize) -> VMResult<[u8; N]> {
        let mut arr = [0u8; N];
        let last = vm_try!(VMResult::from_option(offset.checked_add(N), || {
            VMResult::StackOverflow
        }));
        arr.copy_from_slice(vm_try!(VMResult::from_option(
            self.0.get(offset..last),
            || { VMResult::MemoryIndexOutOfRange }
        )));
        VMResult::Success(arr)
    }
    pub fn init(&mut self, offset: u32, value: &[u8]) -> VMResult<()> {
        let offset = offset as usize;
        let last = vm_try!(VMResult::from_option(
            offset.checked_add(value.len()),
            || { VMResult::MemoryIndexOutOfRange }
        ));
        vm_try!(VMResult::from_option(self.0.get_mut(offset..last), || {
            VMResult::MemoryIndexOutOfRange
        }))
        .copy_from_slice(value);
        VMResult::Success(())
    }
    fn write_slice(&mut self, memarg: MemArg, offset: u32, value: &[u8]) -> VMResult<()> {
        let offset = vm_try!(compute_offset(memarg, offset));
        let n = value.len();
        let last = vm_try!(VMResult::from_option(offset.checked_add(n), || {
            VMResult::MemoryIndexOutOfRange
        }));
        vm_try!(VMResult::from_option(self.0.get_mut(offset..last), || {
            VMResult::MemoryIndexOutOfRange
        }))
        .copy_from_slice(value);
        VMResult::Success(())
    }
    pub fn write_f32(&mut self, memarg: MemArg, offset: u32, value: f32) -> VMResult<()> {
        self.write_slice(memarg, offset, &value.to_le_bytes())
    }
    pub fn write_f64(&mut self, memarg: MemArg, offset: u32, value: f64) -> VMResult<()> {
        self.write_slice(memarg, offset, &value.to_le_bytes())
    }
    pub fn write_u32(&mut self, memarg: MemArg, offset: u32, value: u32) -> VMResult<()> {
        self.write_slice(memarg, offset, &value.to_le_bytes())
    }
    pub fn write_u64(&mut self, memarg: MemArg, offset: u32, value: u64) -> VMResult<()> {
        self.write_slice(memarg, offset, &value.to_le_bytes())
    }
    pub fn write_u8(&mut self, memarg: MemArg, offset: u32, value: u8) -> VMResult<()> {
        *vm_try!(VMResult::from_option(
            self.0.get_mut(vm_try!(compute_offset(memarg, offset))),
            || VMResult::MemoryIndexOutOfRange
        )) = value;

        VMResult::Success(())
    }
    pub fn write_u16(&mut self, memarg: MemArg, offset: u32, value: u16) -> VMResult<()> {
        vm_try!(self.write_slice(memarg, offset, &value.to_le_bytes()));
        VMResult::Success(())
    }
    pub fn read_i32(&self, memarg: MemArg, offset: u32) -> VMResult<i32> {
        VMResult::Success(i32::from_le_bytes(vm_try!(
            self.read_u8_array::<4>(vm_try!(compute_offset(memarg, offset)))
        )))
    }
    pub fn read_u32(&self, memarg: MemArg, offset: u32) -> VMResult<u32> {
        VMResult::Success(u32::from_le_bytes(vm_try!(
            self.read_u8_array::<4>(vm_try!(compute_offset(memarg, offset)))
        )))
    }
    pub fn read_u64(&self, memarg: MemArg, offset: u32) -> VMResult<u64> {
        VMResult::Success(u64::from_le_bytes(vm_try!(
            self.read_u8_array::<8>(vm_try!(compute_offset(memarg, offset)))
        )))
    }
    pub fn read_f32(&self, memarg: MemArg, offset: u32) -> VMResult<f32> {
        VMResult::Success(f32::from_le_bytes(vm_try!(
            self.read_u8_array(vm_try!(compute_offset(memarg, offset)))
        )))
    }
    pub fn read_f64(&self, memarg: MemArg, offset: u32) -> VMResult<f64> {
        VMResult::Success(f64::from_le_bytes(vm_try!(
            self.read_u8_array(vm_try!(compute_offset(memarg, offset)))
        )))
    }
    pub fn read_u8(&self, memarg: MemArg, offset: u32) -> VMResult<u8> {
        VMResult::Success(
            vm_try!(self.read_u8_array::<1>(vm_try!(compute_offset(memarg, offset))))[0],
        )
    }
    pub fn read_i8(&self, memarg: MemArg, offset: u32) -> VMResult<i8> {
        VMResult::Success(
            vm_try!(self.read_u8_array::<1>(vm_try!(compute_offset(memarg, offset))))[0] as i8,
        )
    }
    pub fn read_i16(&self, memarg: MemArg, offset: u32) -> VMResult<i16> {
        VMResult::Success(i16::from_le_bytes(vm_try!(
            self.read_u8_array(vm_try!(compute_offset(memarg, offset)))
        )))
    }
    pub fn read_u16(&self, memarg: MemArg, offset: u32) -> VMResult<u16> {
        VMResult::Success(u16::from_le_bytes(vm_try!(
            self.read_u8_array(vm_try!(compute_offset(memarg, offset)))
        )))
    }
    pub fn page_size(&self) -> u32 {
        (self.0.len() / PAGE_SIZE) as u32
    }
    pub fn grow(&mut self, page_size_delta: u32) -> VMResult<()> {
        // FIXME: check memory allocation and new length
        self.0
            .resize((self.page_size() + page_size_delta) as usize * PAGE_SIZE, 0);
        VMResult::Success(())
    }
    pub fn fill(&mut self, ptr: u32, len: u32, data: u32) -> VMResult<()> {
        let last = vm_try!(VMResult::from_option(ptr.checked_add(len), || {
            VMResult::MemoryIndexOutOfRange
        }));
        let slice = vm_try!(VMResult::from_option(
            self.0.get_mut(ptr as usize..last as usize),
            || { VMResult::MemoryIndexOutOfRange }
        ));

        slice.fill(vm_try!(VMResult::from_option(data.try_into().ok(), || {
            VMResult::Unreachable
        })));
        VMResult::Success(())
    }
    pub fn copy(&mut self, dst: u32, src: u32, len: u32) -> VMResult<()> {
        let src_last = vm_try!(VMResult::from_option(src.checked_add(len), || {
            VMResult::MemoryIndexOutOfRange
        })) as usize;
        if src_last > self.0.len() {
            return VMResult::MemoryIndexOutOfRange;
        }
        let dst_last = vm_try!(VMResult::from_option(dst.checked_add(len), || {
            VMResult::MemoryIndexOutOfRange
        })) as usize;
        if dst_last > self.0.len() {
            return VMResult::MemoryIndexOutOfRange;
        }
        self.0.copy_within(src as usize..src_last, dst as usize);

        VMResult::Success(())
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
#[derive(Debug)]
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
    pub fn ret(&mut self) -> u32 {
        unsafe { self.0.drain(0..).next().unwrap_unchecked() }
    }
    pub fn end(&mut self) {
        self.0.pop();
    }
}
pub struct Stack {
    memory: Box<[u8]>,
    top: usize,
}
impl Debug for Stack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Stack({},{:?})", self.top, &self.memory[0..self.top])
    }
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
    fn add_top(&mut self, n: usize) -> VMResult<()> {
        self.top = vm_try!(VMResult::from_option(self.top.checked_add(n), || {
            VMResult::StackOverflow
        }));
        VMResult::Success(())
    }
    fn sub_top(&mut self, n: usize) {
        self.top -= n;
    }
    fn get_memory(&mut self, n: usize) -> VMResult<&mut [u8]> {
        let last = vm_try!(VMResult::from_option(self.top.checked_add(n), || {
            VMResult::StackOverflow
        }));
        VMResult::Success(vm_try!(VMResult::from_option(
            self.memory.get_mut(self.top..last),
            || VMResult::StackOverflow
        )))
    }
    pub fn push_u8_array<const N: usize>(&mut self, v: [u8; N]) -> VMResult<()> {
        unsafe { std::ptr::copy(v.as_ptr(), vm_try!(self.get_memory(N)).as_mut_ptr(), N) };
        self.add_top(N)
    }

    pub fn push_slice(&mut self, v: &[u8]) -> VMResult<()> {
        unsafe {
            std::ptr::copy(
                v.as_ptr(),
                vm_try!(self.get_memory(v.len())).as_mut_ptr(),
                v.len(),
            )
        };
        self.add_top(v.len())
    }
    pub fn pop_u8_array<const N: usize>(&mut self) -> [u8; N] {
        self.sub_top(N);
        let mut arr = [0u8; N];
        unsafe { std::ptr::copy(self.memory.as_ptr().add(self.top), arr.as_mut_ptr(), N) };
        arr
    }
    pub fn pop_u8_array_generic<const N: usize>(&mut self, n: usize) -> [u8; N] {
        self.sub_top(n);

        let mut arr = [0u8; N];
        unsafe { std::ptr::copy(self.memory.as_ptr().add(self.top), arr.as_mut_ptr(), N) };
        arr
    }
    pub fn drop(&mut self, n: usize) -> &[u8] {
        self.sub_top(n);
        let slice = &self.memory[self.top..self.top + n];
        slice
    }
    pub fn push_u32(&mut self, v: u32) -> VMResult<()> {
        self.push_u8_array(v.to_le_bytes())
    }
    pub fn pop_u32(&mut self) -> u32 {
        u32::from_le_bytes(self.pop_u8_array::<4>())
    }
    pub fn push_u64(&mut self, v: u64) -> VMResult<()> {
        self.push_u8_array(v.to_le_bytes())
    }
    pub fn pop_u64(&mut self) -> u64 {
        u64::from_le_bytes(self.pop_u8_array::<8>())
    }
    pub fn push_i32(&mut self, v: i32) -> VMResult<()> {
        self.push_u8_array(v.to_le_bytes())
    }
    pub fn push_f32(&mut self, v: f32) -> VMResult<()> {
        self.push_u8_array(v.to_le_bytes())
    }
    pub fn push_f64(&mut self, v: f64) -> VMResult<()> {
        self.push_u8_array(v.to_le_bytes())
    }
    pub fn pop_i32(&mut self) -> i32 {
        i32::from_le_bytes(self.pop_u8_array::<4>())
    }
    pub fn push_i64(&mut self, v: i64) -> VMResult<()> {
        self.push_u8_array(v.to_le_bytes())
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
    pub fn local_get(
        &mut self,
        reference: &LocalReference,
        local_addr: usize,
        size: usize,
    ) -> VMResult<()> {
        let new_top = vm_try!(VMResult::from_option(self.top.checked_add(size), || {
            VMResult::StackOverflow
        }));
        if new_top >= self.memory.len() {
            return VMResult::StackOverflow;
        }
        self.memory.copy_within(
            reference.local_top + local_addr..reference.local_top + local_addr + size,
            self.top,
        );
        self.top = new_top;
        VMResult::Success(())
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
    ) -> VMResult<LocalReference> {
        let local_top = vm_try!(VMResult::from_option(
            self.top.checked_sub(param_size),
            || VMResult::StackOverflow
        ));
        vm_try!(self.add_top(local_size));
        vm_try!(self.push_slice(&(return_addr as usize).to_le_bytes()));
        VMResult::Success(LocalReference {
            local_top,
            local_size: param_size + local_size + std::mem::size_of_val(&return_addr),
        })
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
    pub fn block_return(
        &mut self,
        reference: &LocalReference,
        stack_top: usize,
        return_size: usize,
    ) {
        self.memory.copy_within(
            self.top - return_size..self.top,
            reference.local_top + reference.local_size + stack_top,
        );
        self.top = reference.local_top + reference.local_size + stack_top + return_size;
    }
}
