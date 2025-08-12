use std::{future::Future, ops::Range, pin::Pin};

use crate::{
    Stack,
    common::{GcRef, Instr},
};

use super::scheduler::AsyncResult;
#[derive(Debug)]
pub enum Target {
    Memory(GcRef, Range<usize>),
}
#[derive(Debug)]
pub enum AtomicFlag {
    NonAtomic,
}
pub type ReadOperationHandler = unsafe fn(&mut Stack, &[u8], *const Instr) -> *const Instr;
#[derive(Debug)]
pub enum Operation {
    Read(ReadOperationHandler),
    Write(WriteOperation),
}
#[derive(Debug)]
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
#[derive(Debug)]
pub struct MemoryEffect {
    pub task_id: u32,
    pub target: Target,
    pub atomic: AtomicFlag,
    pub operation: Operation,
}
pub type AsyncEffectOperationCallSignature =
    fn(u32, *const Instr) -> Pin<Box<dyn Future<Output = AsyncResult>>>;
#[cfg(feature = "async-runtime")]
#[derive(Debug)]
pub enum AsyncEffectOperation {
    // TODO:
    #[allow(unused)]
    Call(AsyncEffectOperationCallSignature),
}
#[cfg(feature = "async-runtime")]
#[derive(Debug)]
pub struct AsyncEffect {
    pub task_id: u32,
    pub operation: AsyncEffectOperation,
}

#[derive(Debug)]
pub enum Effect {
    MemoryEffect(MemoryEffect),
    #[cfg(feature = "async-runtime")]
    // TODO:
    #[allow(unused)]
    AsyncEffect(AsyncEffect),
}
