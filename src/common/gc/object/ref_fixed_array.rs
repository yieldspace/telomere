use crate::common::gc::{GcView, GcRef, MemoryPool};

#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub struct GcRefFixedArray(pub(crate) GcRef);
impl GcRefFixedArray {
    pub fn len(&self, pool: &MemoryPool) -> u16 {
        if self.0.is_null() {
            return 0;
        }
        pool.read_header(self.0).word_size()
    }
    pub fn as_ptr(&self, pool: &MemoryPool) -> *const GcRef {
        if self.0.is_null() {
            return std::ptr::null();
        }
        pool.memory
            .as_ptr()
            .wrapping_add(self.0.get_value_addr_usize()) as *const GcRef
    }
    pub fn as_ptr_mut(&self, pool: &mut MemoryPool) -> *mut GcRef {
        if self.0.is_null() {
            return std::ptr::null_mut();
        }
        pool.memory
            .as_mut_ptr()
            .wrapping_add(self.0.get_value_addr_usize()) as *mut GcRef
    }
    pub fn as_slice(&self, pool: &MemoryPool) -> &[GcRef] {
        if self.0.is_null() {
            return &[];
        }
        unsafe { std::slice::from_raw_parts(self.as_ptr(pool), self.len(pool).into()) }
    }
    pub fn as_slice_mut(&self, pool: &mut MemoryPool) -> &mut [GcRef] {
        if self.0.is_null() {
            return &mut [];
        }
        unsafe { std::slice::from_raw_parts_mut(self.as_ptr_mut(pool), self.len(pool).into()) }
    }
}
impl GcView for GcRefFixedArray {
    fn trace(&self, pool: &mut MemoryPool) {
        self.0.trace(pool);
        for v in self.as_slice(pool) {
            v.trace(pool);
        }
    }

    fn update(&mut self, pool: &mut MemoryPool) {
        for v in self.as_slice_mut(pool) {
            v.update(pool);
        }
        self.0.update(pool);
    }
}
