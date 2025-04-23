use std::{cell::RefCell, rc::Rc};

use super::{GcRef, MemoryPool};

#[derive(Debug)]
pub struct GcRootHandle{
    pool: Rc<RefCell<MemoryPool>>,
    ptr: GcRef
}
impl GcRootHandle{
    pub fn new(ptr: GcRef,pool: Rc<RefCell<MemoryPool>> )-> Self{
        pool.borrow_mut().add_root(&[ptr]);
        Self { pool, ptr }
    }
    pub fn get_inner(&self) -> GcRef{
        self.ptr
    }
}
impl Drop for GcRootHandle{
    fn drop(&mut self) {
        self.pool.borrow_mut().remove_root(&[self.ptr]);
    }
}