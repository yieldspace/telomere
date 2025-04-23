use std::{cell::RefCell, rc::Rc};

use super::{GcRef, GcRefDynamicArray, MemoryPool};

#[derive(Debug)]
pub struct GcRootHandle {
    pool: Rc<RefCell<MemoryPool>>,
    idx: u32,
}
impl GcRootHandle {
    pub fn new(ptr: GcRef, pool: Rc<RefCell<MemoryPool>>) -> Self {
        Self::new_with_ref(ptr, &mut pool.borrow_mut(), pool.clone())
        
    }
    pub fn new_with_ref(
        ptr: GcRef,
        pool_ref: &mut MemoryPool,
        pool: Rc<RefCell<MemoryPool>>,
    ) -> Self {
        let idx = pool_ref.add_root(ptr);
        Self { pool, idx }
    }
    pub fn get_gc_ref_with_pool(&self, pool_ref: &MemoryPool) -> GcRef {
        let arr = unsafe {
            pool_ref
                .get_value::<GcRefDynamicArray>(GcRef(1), 0)
                .as_ref()
                .unwrap()
        };
        arr.as_slice(pool_ref)[self.idx as usize]
    }
}
impl Drop for GcRootHandle {
    fn drop(&mut self) {
        self.pool.borrow_mut().remove_root(self.idx);
    }
}
