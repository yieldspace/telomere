use parking_lot::Mutex;
use std::{num::NonZeroU32, sync::Weak};

use super::MemoryPool;

#[derive(Debug)]
pub struct GcRootHandle {
    pub(crate) pool: Weak<Mutex<MemoryPool>>,
    pub(crate) store_identity: Weak<()>,
    pub(crate) slot: NonZeroU32,
}

impl GcRootHandle {
    pub fn new(slot: u32, pool: Weak<Mutex<MemoryPool>>, store_identity: Weak<()>) -> Self {
        Self {
            pool,
            store_identity,
            slot: NonZeroU32::new(slot).expect("root slot ids are 1-based"),
        }
    }
}

impl Drop for GcRootHandle {
    fn drop(&mut self) {
        if crate::common::store::clear_active_root_slot_for_identity(
            &self.store_identity,
            self.slot.get(),
        ) {
            return;
        }
        if let Some(pool) = self.pool.upgrade() {
            pool.lock().remove_root(self.slot.get());
        }
    }
}
