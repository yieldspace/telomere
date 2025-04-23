use crate::common::gc::{GcView, MemoryPool};

use super::U32FixedArray;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct U32DynamicArray {
    pub(crate) len: u32,
    pub(crate) array: U32FixedArray,
}
// TODO:
#[allow(dead_code)]
impl U32DynamicArray {
    pub fn len(&self, _pool: &MemoryPool) -> u16 {
        self.len as u16
    }
    pub fn cap(&self, pool: &MemoryPool) -> u16 {
        self.array.len(pool)
    }
    pub fn as_ptr(&self, pool: &MemoryPool) -> *const u32 {
        self.array.as_ptr(pool)
    }
    pub fn as_slice(&self, pool: &MemoryPool) -> &[u32] {
        unsafe { std::slice::from_raw_parts(self.as_ptr(pool), self.len(pool).into()) }
    }
}
impl GcView for U32DynamicArray {
    fn trace(&self, pool: &mut MemoryPool) {
        self.array.trace(pool);
    }
    fn update(&mut self, pool: &mut MemoryPool) {
        self.array.update(pool);
    }
}
