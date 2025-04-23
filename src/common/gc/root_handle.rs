use std::{cell::RefCell, rc::Rc};

use super::{GcRef, GcRefDynamicArray, MemoryPool};

#[derive(Debug)]
pub struct GcRootHandle {
    pool: Rc<RefCell<MemoryPool>>,
    idx: u32,
}
impl GcRootHandle {
    pub fn new(ptr: GcRef, pool: Rc<RefCell<MemoryPool>>) -> Self {
        let idx = pool.borrow_mut().add_root(ptr);
        Self { pool, idx }
    }
    pub fn into_inner(&self) -> GcRef {
        let arr = unsafe {
            self.pool
                .borrow()
                .get_value::<GcRefDynamicArray>(GcRef(1), 0)
                .as_ref()
                .unwrap()
        };
        arr.as_slice(&self.pool.borrow())[self.idx as usize]
    }
}
impl Drop for GcRootHandle {
    fn drop(&mut self) {
        self.pool.borrow_mut().remove_root(self.idx);
    }
}
