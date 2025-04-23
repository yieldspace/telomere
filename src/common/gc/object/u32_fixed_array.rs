use crate::common::gc::{GcRef, GcView, MemoryPool};

#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub struct U32FixedArray(pub(crate) GcRef);

impl GcView for U32FixedArray {
    fn trace(&self, pool: &mut MemoryPool) {
        self.0.trace(pool);
    }

    fn update(&mut self, pool: &mut MemoryPool) {
        self.0.update(pool);
    }
}
