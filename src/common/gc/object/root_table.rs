use crate::common::gc::{GCView, MemoryPool};

use super::GcRefDynamicArray;

pub(crate) struct RootTable {
    roots: GcRefDynamicArray,
}
impl GCView for RootTable {
    fn trace(&self, pool: &mut MemoryPool) {
        self.roots.trace(pool);
    }
    fn update(&mut self, pool: &mut MemoryPool) {
        self.roots.update(pool);
    }
}
