use super::{
    gc::{GcRef, MemoryPool},
    Data, Elem, ExportSection, FuncType, GlobalType, MemType, TableType, TypeIdx,
};
use parking_lot::{Mutex, MutexGuard};
use std::{
    cell::RefCell,
    collections::HashMap,
    ops::{Deref, DerefMut},
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Weak,
    },
};

thread_local! {
    static ACTIVE_STORE_GC: RefCell<Vec<(*const (), *mut MemoryPool)>> = const { RefCell::new(Vec::new()) };
}

#[derive(Debug)]
pub struct ModuleInstance {
    pub exports: ExportSection,
    pub tables: Vec<TableType>,
    pub globals: Vec<GlobalType>,
    pub functions: Vec<TypeIdx>,
    pub function_types: Vec<FuncType>,
    pub mems: Vec<MemType>,
}

#[derive(Default)]
pub struct StoreSegments {
    pub data: HashMap<(u32, u32), Data>,
    pub elems: HashMap<(u32, u32), Elem>,
}

pub struct Store {
    gc: Arc<Mutex<MemoryPool>>,
    identity: Arc<()>,
    segments: Mutex<StoreSegments>,
    next_instance_id: AtomicU32,
    pub state: StoreState,
}
impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}

pub struct StoreGcGuard<'a> {
    guard: MutexGuard<'a, MemoryPool>,
    identity: *const (),
}

impl<'a> StoreGcGuard<'a> {
    fn new(identity: &Arc<()>, mut guard: MutexGuard<'a, MemoryPool>) -> Self {
        let identity_ptr = Arc::as_ptr(identity);
        let gc_ptr = (&mut *guard) as *mut MemoryPool;
        ACTIVE_STORE_GC.with(|active| active.borrow_mut().push((identity_ptr, gc_ptr)));
        Self {
            guard,
            identity: identity_ptr,
        }
    }
}

impl Deref for StoreGcGuard<'_> {
    type Target = MemoryPool;

    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}

impl DerefMut for StoreGcGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.guard
    }
}

impl Drop for StoreGcGuard<'_> {
    fn drop(&mut self) {
        ACTIVE_STORE_GC.with(|active| {
            let (identity, _) = active
                .borrow_mut()
                .pop()
                .expect("store GC guard stack must stay balanced");
            debug_assert_eq!(identity, self.identity);
        });
    }
}

impl Store {
    pub fn new() -> Self {
        Self::new_with_state(StoreState::default())
    }

    pub fn new_with_state(state: StoreState) -> Self {
        Store {
            gc: Arc::new(Mutex::new(MemoryPool::new())),
            identity: Arc::new(()),
            segments: Mutex::new(StoreSegments::default()),
            next_instance_id: AtomicU32::new(1),
            state,
        }
    }

    pub fn lock_gc(&self) -> StoreGcGuard<'_> {
        StoreGcGuard::new(&self.identity, self.gc.lock())
    }

    pub fn lock_segments(&self) -> MutexGuard<'_, StoreSegments> {
        self.segments.lock()
    }

    pub fn with_gc<T>(&self, f: impl FnOnce(&mut MemoryPool) -> T) -> T {
        let mut gc = self.lock_gc();
        f(&mut gc)
    }

    pub fn with_segments<T>(&self, f: impl FnOnce(&mut StoreSegments) -> T) -> T {
        let mut segments = self.lock_segments();
        f(&mut segments)
    }

    pub(crate) fn new_instance_id(&self) -> u32 {
        self.next_instance_id.fetch_add(1, Ordering::Relaxed)
    }

    pub(crate) fn gc_weak(&self) -> Weak<Mutex<MemoryPool>> {
        Arc::downgrade(&self.gc)
    }

    pub(crate) fn identity_weak(&self) -> Weak<()> {
        Arc::downgrade(&self.identity)
    }

    pub(crate) fn matches_identity(&self, identity: &Weak<()>) -> bool {
        identity
            .upgrade()
            .is_some_and(|identity| Arc::ptr_eq(&self.identity, &identity))
    }

    pub(crate) fn has_active_gc_on_current_thread(&self) -> bool {
        has_active_gc_for_identity(&self.identity_weak())
    }
}

fn active_gc_ptr_for_identity(identity: &Weak<()>) -> Option<*mut MemoryPool> {
    let identity = identity.upgrade()?;
    let identity_ptr = Arc::as_ptr(&identity);
    ACTIVE_STORE_GC.with(|active| {
        let active = active.borrow();
        active
            .iter()
            .rev()
            .find_map(|(active_identity, gc)| (*active_identity == identity_ptr).then_some(*gc))
    })
}

pub(crate) fn has_active_gc_for_identity(identity: &Weak<()>) -> bool {
    active_gc_ptr_for_identity(identity).is_some()
}

pub(crate) fn clear_active_root_slot_for_identity(identity: &Weak<()>, slot: u32) -> bool {
    let Some(gc) = active_gc_ptr_for_identity(identity) else {
        return false;
    };
    unsafe {
        (*gc).write_root_slot(slot, GcRef(0));
    }
    true
}

#[derive(Default)]
pub struct StoreState(usize);

impl StoreState {
    pub const fn empty() -> Self {
        StoreState(0)
    }

    pub fn from_static<T>(data: &'static T) -> Self
    where
        T: Sync,
    {
        // SAFETY: a `'static` reference is valid for the lifetime of the store.
        unsafe { Self::from_ptr(data as *const T) }
    }

    /// # Safety
    ///
    /// `data` must remain valid for the entire time the store may expose this state,
    /// and the pointed-to value must be safe to share across threads.
    pub unsafe fn from_ptr<T>(data: *const T) -> Self
    where
        T: Sync,
    {
        StoreState(data.cast::<()>() as usize)
    }

    #[inline]
    /// # Safety
    ///
    /// The stored pointer must either be null or point to a live value of type `T`
    /// for the duration of the returned reference.
    pub unsafe fn get<T>(&self) -> Option<&T> {
        let ptr = self.0 as *const T;
        ptr.as_ref()
    }
}

#[cfg(test)]
mod test {
    #[test]
    fn test_state() {
        use super::StoreState;
        static DATA: [i32; 3] = [1, 2, 3];
        let state = StoreState::from_static(&DATA);
        let value = unsafe { state.get::<[i32; 3]>() }.unwrap();
        assert_eq!(value, &DATA);
    }
}
