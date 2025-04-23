use crate::common::gc::{GcView, GcRef, MemoryPool};

use super::{GcRefFixedArray, U32FixedArray};

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct InstanceData {
    pub instance_id: u32,
    pub module_addr: GcRef,
    pub globals: U32FixedArray,
    pub funcs: U32FixedArray,
    pub tables: GcRefFixedArray,
    pub mems: GcRefFixedArray,
}
impl GcView for InstanceData {
    fn trace(&self, pool: &mut MemoryPool) {
        self.globals.trace(pool);
        self.funcs.trace(pool);
        self.tables.trace(pool);
        self.mems.trace(pool);
    }

    fn update(&mut self, pool: &mut MemoryPool) {
        self.mems.update(pool);
        // do nothing
    }
}
