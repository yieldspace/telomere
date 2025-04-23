use super::{MemoryPool, HEADER_LEN};

// GC のトレース用トレイト
pub trait GCView {
    fn trace(&self, pool: &mut MemoryPool);
    fn update(&mut self, pool: &mut MemoryPool);
}

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

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct InstanceData {
    pub instance_id: u32,
    pub module_addr: u32,
    pub globals: U32FixedArray,
    pub funcs: U32FixedArray,
    pub tables: U32FixedArray,
    pub mems: GcRefFixedArray,
}
impl GCView for InstanceData {
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
impl GCView for U32FixedArray {
    fn trace(&self, pool: &mut MemoryPool) {
        self.0.trace(pool);
    }

    fn update(&mut self, pool: &mut MemoryPool) {
        self.0.update(pool);
    }
}
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
impl GCView for U32DynamicArray {
    fn trace(&self, pool: &mut MemoryPool) {
        self.array.trace(pool);
    }
    fn update(&mut self, pool: &mut MemoryPool) {
        self.array.update(pool);
    }
}
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
impl GCView for GcRefFixedArray {
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
pub(crate) struct RootTable {
    roots: GcRefDynamicArray,
}
impl GCView for RootTable {
    fn trace(&self, pool: &mut MemoryPool) {
        self.roots.trace(pool);
    }
    fn update(&mut self, pool: &mut MemoryPool) {
        self.roots.update(pool);
    }
}
