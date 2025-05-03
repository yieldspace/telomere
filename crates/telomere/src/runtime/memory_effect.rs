use std::ops::Range;

use crate::{
    common::{GcRef, Instr},
    Stack, VMResult,
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
    Write,
}
pub struct Effect {
    pub task_id: u32,
    pub target: Target,
    pub atomic: AtomicFlag,
    pub operation: Operation,
}
