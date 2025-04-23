use crate::common::gc::{GCView, GcRef, MemoryPool};

use super::GcRefFixedArray;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GcRefDynamicArray {
    pub(crate) len: u32,
    pub(crate) array: GcRefFixedArray,
}
impl GcRefDynamicArray {
    pub fn len(&self, _pool: &MemoryPool) -> u16 {
        self.len as u16
    }
    pub fn cap(&self, pool: &MemoryPool) -> u16 {
        self.array.len(pool)
    }
    pub fn as_ptr(&self, pool: &MemoryPool) -> *const GcRef {
        self.array.as_ptr(pool)
    }
    pub fn as_ptr_mut(&self, pool: &mut MemoryPool) -> *mut GcRef {
        self.array.as_ptr_mut(pool)
    }
    pub fn as_slice(&self, pool: &MemoryPool) -> &[GcRef] {
        if self.len == 0 {
            return &[];
        }
        unsafe { std::slice::from_raw_parts(self.as_ptr(pool), self.len(pool).into()) }
    }
    pub fn as_slice_mut(&self, pool: &mut MemoryPool) -> &mut [GcRef] {
        if self.len == 0 {
            return &mut [];
        }
        unsafe { std::slice::from_raw_parts_mut(self.as_ptr_mut(pool), self.len(pool).into()) }
    }
}
impl GCView for GcRefDynamicArray {
    fn trace(&self, pool: &mut MemoryPool) {
        pool.mark(self.array.0);
        for v in self.as_slice(pool) {
            v.trace(pool);
        }
    }
    fn update(&mut self, pool: &mut MemoryPool) {
        tracing::trace!("dynamic array update: {:?}", self);
        for v in self.as_slice_mut(pool) {
            tracing::trace!("dynamic array update: {:?}", v);
            v.update(pool);
        }
        self.array.0.update(pool);
    }
}
