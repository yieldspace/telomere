use crate::common::gc::GcRef;
use crate::common::gc::GcView;
use crate::common::gc::MemoryPool;

pub struct Global4Data {
    pub global4: u32,
}
impl GcView for Global4Data {
    fn trace(&self, _pool: &mut MemoryPool) {}
    fn update(&mut self, _pool: &mut MemoryPool) {}
}
impl Global4Data {
    pub fn new(global4: u32) -> Self {
        Self { global4 }
    }
}

pub struct GlobalRefData {
    pub global_ref: GcRef,
}

impl GcView for GlobalRefData {
    fn trace(&self, pool: &mut MemoryPool) {
        pool.trace(self.global_ref);
    }
    fn update(&mut self, pool: &mut MemoryPool) {
        if !self.global_ref.is_null() {
            self.global_ref.update(pool);
        }
    }
}
impl GlobalRefData {
    pub fn new(global_ref: GcRef) -> Self {
        Self { global_ref }
    }
}

pub struct Global8Data {
    pub global8: u64,
}
impl GcView for Global8Data {
    fn trace(&self, _pool: &mut MemoryPool) {}
    fn update(&mut self, _pool: &mut MemoryPool) {}
}
impl Global8Data {
    pub fn new(global8: u64) -> Self {
        Self { global8 }
    }
}
