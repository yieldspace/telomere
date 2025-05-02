use std::ops::Range;

use crate::common::GcRef;

pub enum Target{
  Memory(GcRef,Range<u32>),
  Table(GcRef,u32),
  Global(GcRef),
}
pub enum AtomicFlag{
  Atomic,
  NonAtomic
}
pub enum Operation{
  Read,Write
}
pub struct Effect{
  pub task_id: u32,
  pub target: Target,
  pub atomic: AtomicFlag,
  pub operation: Operation
}