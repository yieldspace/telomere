use crate::common::gc::{GcView, MemoryPool};

use super::GcRefDynamicArray;

pub(crate) struct RootTable {
    roots: GcRefDynamicArray,
}
impl GcView for RootTable {
    fn trace(&self, pool: &mut MemoryPool) {
        self.roots.trace(pool);
    }
    fn update(&mut self, pool: &mut MemoryPool) {
        self.roots.update(pool);
    }
}
