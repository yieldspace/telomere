use std::slice;

use super::{Header, MemoryPool, ObjectType};

#[repr(transparent)]
#[derive(Debug, Copy, Clone)]
pub struct GcRef(pub u32);

impl GcRef {
    pub fn get(&self) -> u32 {
        self.0
    }
    pub fn get_value_addr(&self) -> u32 {
        self.0 + 1
    }
    pub fn get_usize(&self) -> usize {
        self.0 as usize
    }
    pub fn get_value_addr_usize(&self) -> usize {
        self.get_usize() + 1
    }
}

// GC のトレース用トレイト
pub trait GCView {
    fn trace(&self, pool: &mut MemoryPool);
    fn word_size(&self) -> usize;
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct InstanceData {
    pub instance_id: u32,
    pub module_addr: u32,
    pub globals: U32FixedArray,
    pub funcs: U32FixedArray,
    pub tables: U32FixedArray,
    pub mems: U32FixedArray,
}
#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub struct U32FixedArray(pub(crate) GcRef);
impl U32FixedArray {
    pub fn size(&self, pool: &MemoryPool) -> u16 {
        pool.read_header(self.0).word_size()
    }
    pub fn as_ptr(&self, pool: &MemoryPool) -> *const u32 {
        pool.memory
            .as_ptr()
            .wrapping_add(self.0.get_value_addr_usize())
    }
    pub fn as_slice(&self, pool: &MemoryPool) -> &[u32] {
        unsafe { std::slice::from_raw_parts(self.as_ptr(pool), self.size(pool).into()) }
    }
}
