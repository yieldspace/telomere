use std::slice;

use super::{word_size, Header, MemoryPool, ObjectType};

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
impl GCView for GcRef {
    fn trace(&self, pool: &mut MemoryPool) {
        pool.trace(*self);
    }

    fn size(&self, _pool: &MemoryPool) -> u16 {
        word_size::<Self>() as u16
    }
}
// GC のトレース用トレイト
pub trait GCView {
    fn trace(&self, pool: &mut MemoryPool);
    fn size(&self, pool: &MemoryPool) -> u16;
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
impl GCView for InstanceData {
    fn trace(&self, pool: &mut MemoryPool) {
        self.globals.trace(pool);
        self.funcs.trace(pool);
        self.tables.trace(pool);
        self.mems.trace(pool);
    }

    fn size(&self, _pool: &MemoryPool) -> u16 {
        word_size::<Self>() as u16
    }
}
#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub struct U32FixedArray(pub(crate) GcRef);
impl U32FixedArray {
    pub fn len(&self, pool: &MemoryPool) -> u16 {
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
impl GCView for U32FixedArray {
    fn trace(&self, pool: &mut MemoryPool) {
        self.0.trace(pool);
    }

    fn size(&self, pool: &MemoryPool) -> u16 {
        pool.read_header(self.0).word_size()
    }
}
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct U32DynamicArray {
    pub(crate) len: u32,
    pub(crate) array: U32FixedArray,
}
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
        unsafe { std::slice::from_raw_parts(self.as_ptr(pool), self.size(pool).into()) }
    }
    pub fn push(&mut self, v: &[u32], pool: &mut MemoryPool) {
        unsafe { pool.u32_array_push_vec(self, v) };
    }
}
impl GCView for U32DynamicArray {
    fn trace(&self, pool: &mut MemoryPool) {
        self.array.trace(pool);
    }

    fn size(&self, pool: &MemoryPool) -> u16 {
        pool.read_header(self.array.0).word_size()
    }
}
#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub struct GcRefFixedArray(pub(crate) GcRef);
impl GcRefFixedArray {
    pub fn len(&self, pool: &MemoryPool) -> u16 {
        self.size(pool)
    }
    pub fn as_ptr(&self, pool: &MemoryPool) -> *const GcRef {
        pool.memory
            .as_ptr()
            .wrapping_add(self.0.get_value_addr_usize()) as *const GcRef
    }
    pub fn as_slice(&self, pool: &MemoryPool) -> &[GcRef] {
        unsafe { std::slice::from_raw_parts(self.as_ptr(pool), self.size(pool).into()) }
    }
}
impl GCView for GcRefFixedArray {
    fn trace(&self, pool: &mut MemoryPool) {
        self.0.trace(pool);
        for v in self.as_slice(pool) {
            v.trace(pool);
        }
    }

    fn size(&self, pool: &MemoryPool) -> u16 {
        pool.read_header(self.0).word_size()
    }
}
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
        self.array.size(pool)
    }
    pub fn as_ptr(&self, pool: &MemoryPool) -> *const GcRef {
        self.array.as_ptr(pool)
    }
    pub fn as_slice(&self, pool: &MemoryPool) -> &[GcRef] {
        unsafe { std::slice::from_raw_parts(self.as_ptr(pool), self.len(pool).into()) }
    }
    pub fn push(&mut self, v: &[GcRef], pool: &mut MemoryPool) {
        unsafe { pool.gc_ref_array_push_vec(self, v) };
    }
}
impl GCView for GcRefDynamicArray {
    fn trace(&self, pool: &mut MemoryPool) {
        self.array.trace(pool);
        for v in self.as_slice(pool) {
            v.trace(pool);
        }
    }

    fn size(&self, pool: &MemoryPool) -> u16 {
        pool.read_header(self.array.0).word_size()
    }
}
pub struct RootTable(pub(crate) GcRefDynamicArray);
