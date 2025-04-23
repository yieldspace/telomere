use crate::common::gc::{GcView, GcRef, MemoryPool};

#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub struct U32FixedArray(pub(crate) GcRef);
impl U32FixedArray {
    pub fn len(&self, pool: &MemoryPool) -> u16 {
        if self.0.is_null() {
            return 0;
        }
        pool.read_header(self.0).word_size()
    }
    pub fn as_ptr(&self, pool: &MemoryPool) -> *const u32 {
        if self.0.is_null() {
            return std::ptr::null();
        }
        pool.memory
            .as_ptr()
            .wrapping_add(self.0.get_value_addr_usize())
    }
    pub fn as_slice(&self, pool: &MemoryPool) -> &[u32] {
        if self.0.is_null() {
            return &[];
        }
        unsafe { std::slice::from_raw_parts(self.as_ptr(pool), self.len(pool).into()) }
    }
}
impl GcView for U32FixedArray {
    fn trace(&self, pool: &mut MemoryPool) {
        self.0.trace(pool);
    }

    fn update(&mut self, pool: &mut MemoryPool) {
        self.0.update(pool);
    }
}
