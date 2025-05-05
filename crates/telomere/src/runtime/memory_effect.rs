use std::ops::Range;

use crate::{
    common::{GcRef, Instr},
    Stack,
};

pub enum Target {
    Memory(GcRef, Range<usize>),
    Table(GcRef, u32),
    Global(GcRef),
}
pub enum AtomicFlag {
    Atomic,
    NonAtomic,
}
pub type ReadOperationHandler = unsafe fn(&mut Stack, &[u8], *const Instr) -> *const Instr;
pub enum Operation {
    Read(ReadOperationHandler),
    Write(WriteOperation),
}
pub enum WriteOperation {
    Write1([u8; 1]),
    Write2([u8; 2]),
    Write4([u8; 4]),
    Write8([u8; 8]),
    Write16([u8; 16]),
}
impl WriteOperation {
    pub fn get(&self) -> &[u8] {
        match self {
            Self::Write1(d) => d,
            Self::Write2(d) => d,
            Self::Write4(d) => d,
            Self::Write8(d) => d,
            Self::Write16(d) => d,
        }
    }
}
pub struct Effect {
    pub task_id: u32,
    pub target: Target,
    pub atomic: AtomicFlag,
    pub operation: Operation,
}
