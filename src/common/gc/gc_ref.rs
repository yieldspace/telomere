use super::{GCView, MemoryPool, HEADER_LEN};

// NOTE: GcRef will disabled after gc
#[repr(transparent)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct GcRef(pub u32);

impl GcRef {
    pub fn get(&self) -> u32 {
        self.0
    }
    pub fn get_value_addr(&self) -> u32 {
        self.0 + HEADER_LEN as u32
    }
    pub fn get_usize(&self) -> usize {
        self.0 as usize
    }
    pub fn get_value_addr_usize(&self) -> usize {
        self.get_usize() + HEADER_LEN
    }
    pub fn is_null(&self) -> bool {
        self.0 == 0
    }
}
impl GCView for GcRef {
    fn trace(&self, pool: &mut MemoryPool) {
        pool.trace(*self);
    }
    fn update(&mut self, pool: &mut MemoryPool) {
        tracing::trace!("{self:?}");
        if !self.is_null() {
            self.0 = pool.read_header(*self).forwarding_pointer()
        }
    }
}