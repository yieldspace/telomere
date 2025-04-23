use crate::common::gc::GcRef;
use crate::common::gc::GcView;
use crate::common::gc::MemoryPool;
#[repr(transparent)]
pub struct Global4Data {
    pub _global4: u32,
}
impl GcView for Global4Data {
    fn trace(&self, _pool: &mut MemoryPool) {}
    fn update(&mut self, _pool: &mut MemoryPool) {}
}

#[repr(transparent)]
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

#[repr(transparent)]
pub struct Global8Data {
    pub _global8: u64,
}
impl GcView for Global8Data {
    fn trace(&self, _pool: &mut MemoryPool) {}
    fn update(&mut self, _pool: &mut MemoryPool) {}
}
