use std::fmt::Debug;

#[derive(Clone, Copy)]
pub union Operand {
    pub i32: i32,
    pub u32: u32,
    pub i64: i64,
    pub u64: u64,
}
impl Debug for Operand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "operand")
    }
}
pub type Op = fn(Operand, &[Instr], &mut ExecuteContext);
pub const OPERAND_NONE: Operand = Operand { u64: 0 };
#[derive(Debug, Clone)]
pub struct Instr {
    pub op: Op,
    pub operand: Operand,
}
pub struct ExecuteContext<'a> {
    pub code: &'a [Instr],
    pub stack: &'a mut Stack,
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
    pub fn pop_u8_array<const N: usize>(&mut self) -> [u8; N] {
        self.top -= N;
        let mut arr = [0u8; N];
        arr.copy_from_slice(&self.memory[self.top..self.top + N]);
        arr
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
    pub fn pop_i32(&mut self) -> i32 {
        i32::from_le_bytes(self.pop_u8_array::<4>())
    }
    pub fn push_i64(&mut self, v: i64) {
        self.push_u8_array(v.to_le_bytes());
    }
    pub fn pop_i64(&mut self) -> i64 {
        i64::from_le_bytes(self.pop_u8_array::<8>())
    }
}
