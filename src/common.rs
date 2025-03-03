use crate::parser::core::MemArg;

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

pub type Op = fn(&[Instr], &mut ExecuteContext);
pub union Instr {
    pub op: Op,
    pub operand: Operand,
}

pub struct Memory<'a>(&'a mut [u8]);
impl<'a> Memory<'a> {
    pub fn new(inner: &'a mut [u8]) -> Self {
        Self(inner)
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
    pub code: &'a [Instr],
    pub stack: &'a mut Stack,
    // TODO: We should resolve jump address during instantiate time
    pub jump_table: JumpTable,
    pub locals: &'a mut [u8],
    pub globals: &'a mut [u8],
    pub memory: Memory<'a>,
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
        self.memory[self.top..self.top + N].copy_from_slice(&v);
        self.top += N;
    }

    pub fn push_slice(&mut self, v: &[u8]) {
        self.memory[self.top..self.top + v.len()].copy_from_slice(v);
        self.top += v.len();
    }
    pub fn pop_u8_array<const N: usize>(&mut self) -> [u8; N] {
        self.top -= N;
        let mut arr = [0u8; N];
        arr.copy_from_slice(&self.memory[self.top..self.top + N]);
        arr
    }
    pub fn pop_u8_array_generic<const N: usize>(&mut self, n: usize) -> [u8; N] {
        self.top -= n;
        let mut arr = [0u8; N];
        arr[0..n].copy_from_slice(&self.memory[self.top..self.top + n]);
        arr
    }
    pub fn drop(&mut self, n: usize) {
        self.top -= n;
    }
    pub fn push_u32(&mut self, v: u32) {
        self.push_u8_array(v.to_le_bytes());
    }
    pub fn pop_u32(&mut self) -> u32 {
        u32::from_le_bytes(self.pop_u8_array::<4>())
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
}
