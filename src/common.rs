use crate::{parser::core::MemArg, Module};

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
}
pub type Op = unsafe fn(*const Instr, &mut ExecuteContext) -> Result<u32, VMError>;
pub union Instr {
    pub op: Op,
    pub operand: Operand,
}
unsafe impl Send for Instr {}
unsafe impl Sync for Instr {}
pub struct Memory<'a>(pub &'a mut [u8]);
impl<'a> Memory<'a> {
    pub fn new(inner: &'a mut [u8]) -> Self {
        Self(inner)
    }
    pub fn copy(v: &'a mut Self) -> Self {
        Self(v.0)
    }
    pub fn read_u8_array<const N: usize>(&self, offset: usize) -> [u8; N] {
        let mut arr = [0u8; N];
        arr.copy_from_slice(&self.0[offset..offset + N]);
        arr
    }
    pub fn write_u32(&mut self, memarg: MemArg, value: u32) {
        self.0[memarg.offset as usize..(memarg.offset + 4) as usize]
            .copy_from_slice(&value.to_le_bytes());
    }
    pub fn read_u32(&self, memarg: MemArg) -> u32 {
        u32::from_le_bytes(self.read_u8_array::<4>(memarg.offset as usize))
    }
}
pub struct ExecuteContext<'a> {
    pub module: &'a Module,
    pub stack: &'a mut Stack,

    pub globals: &'a mut [u8],
    pub memory: Memory<'a>,
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
    pub local_reference: LocalReference,
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
