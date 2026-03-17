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
            let mut active = active.borrow_mut();
            let index = active
                .iter()
                .rposition(|(identity, _)| *identity == self.identity)
                .expect("store GC guard stack must stay balanced");
            let (identity, _) = active.remove(index);
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

    fn assert_no_same_thread_gc_reentry(&self, api_name: &str) {
        assert!(
            !self.has_active_gc_on_current_thread(),
            "{api_name} is unsupported while the same store GC is already active on this thread"
        );
    }
    fn lock_gc_unchecked(&self) -> StoreGcGuard<'_> {
        StoreGcGuard::new(&self.identity, self.gc.lock())
    }

    pub fn lock_gc(&self) -> StoreGcGuard<'_> {
        self.assert_no_same_thread_gc_reentry("lock_gc");
        self.lock_gc_unchecked()
    }

    pub fn lock_segments(&self) -> MutexGuard<'_, StoreSegments> {
        self.segments.lock()
    }

    pub fn with_gc<T>(&self, f: impl FnOnce(&mut MemoryPool) -> T) -> T {
        self.assert_no_same_thread_gc_reentry("with_gc");
        let mut gc = self.lock_gc_unchecked();
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

#[derive(Default, Clone, Copy)]
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
    use super::{has_active_gc_for_identity, Store, StoreState};
    use std::panic::{self, AssertUnwindSafe};

    fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
        match payload.downcast::<String>() {
            Ok(message) => *message,
            Err(payload) => match payload.downcast::<&'static str>() {
                Ok(message) => (*message).to_owned(),
                Err(_) => "<non-string panic payload>".to_owned(),
            },
        }
    }

    #[test]
    fn test_state() {
        static DATA: [i32; 3] = [1, 2, 3];
        let state = StoreState::from_static(&DATA);
        let value = unsafe { state.get::<[i32; 3]>() }.unwrap();
        assert_eq!(value, &DATA);
    }

    #[test]
    fn non_lifo_store_gc_drop_keeps_other_store_active() {
        let store_a = Store::new();
        let store_b = Store::new();
        let identity_a = store_a.identity_weak();
        let identity_b = store_b.identity_weak();

        let guard_a = store_a.lock_gc();
        let guard_b = store_b.lock_gc();

        assert!(has_active_gc_for_identity(&identity_a));
        assert!(has_active_gc_for_identity(&identity_b));

        drop(guard_a);

        assert!(!has_active_gc_for_identity(&identity_a));
        assert!(has_active_gc_for_identity(&identity_b));

        drop(guard_b);

        assert!(!has_active_gc_for_identity(&identity_b));
    }

    #[test]
    fn lock_gc_panics_on_same_thread_reentry() {
        let store = Store::new();
        let _guard = store.lock_gc();

        let panic = panic::catch_unwind(AssertUnwindSafe(|| {
            let _reentered = store.lock_gc();
        }))
        .expect_err("lock_gc should panic on same-thread same-store reentry");

        assert!(panic_message(panic).contains("lock_gc"));
    }

    #[test]
    fn with_gc_panics_on_same_thread_reentry() {
        let store = Store::new();
        let _guard = store.lock_gc();

        let panic = panic::catch_unwind(AssertUnwindSafe(|| {
            store.with_gc(|_| ());
        }))
        .expect_err("with_gc should panic on same-thread same-store reentry");

        assert!(panic_message(panic).contains("with_gc"));
    }
}
