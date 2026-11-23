use std::{fmt, ptr::NonNull, slice::SliceIndex, sync::Arc};

#[cfg(test)]
use std::cell::Cell;

#[cfg(feature = "threads")]
use std::{
    collections::{HashMap, VecDeque},
    sync::atomic::{AtomicU8, Ordering},
    time::{Duration, Instant},
};

use parking_lot::Mutex;
#[cfg(feature = "threads")]
use tokio::sync::Notify;

#[cfg(feature = "threads")]
use super::MeteringHandle;
#[cfg(test)]
use super::PAGE_SIZE_MAX;
use super::{Stack, VMResult, PAGE_SIZE};

#[inline(always)]
// #202: This helper is only reachable through the feature-gated raw linear-memory ABI.
#[cfg_attr(not(feature = "unstable-internals"), allow(dead_code))]
pub fn checked_memory_offset(memarg_offset: u32, offset: u32) -> Option<usize> {
    let sum = memarg_offset as u64 + offset as u64;
    if sum <= u32::MAX as u64 {
        Some(sum as usize)
    } else {
        None
    }
}

#[inline(always)]
pub fn atomic_alignment_valid(offset: usize, alignment: usize) -> bool {
    debug_assert_ne!(alignment, 0);
    offset % alignment == 0
}

#[inline(always)]
pub fn trusted_copy_from_slice(dst: &mut [u8], src: &[u8]) {
    debug_assert_eq!(dst.len(), src.len());
    dst.copy_from_slice(src);
}

#[inline(always)]
pub fn trusted_fill_slice(dst: &mut [u8], value: u8) {
    dst.fill(value);
}

#[inline(always)]
pub fn trusted_copy_within(dst: &mut [u8], src_start: usize, src_end: usize, dst_start: usize) {
    debug_assert!(src_start <= src_end);
    debug_assert!(src_end <= dst.len());
    debug_assert!(dst_start + (src_end - src_start) <= dst.len());
    dst.copy_within(src_start..src_end, dst_start);
}

#[inline(always)]
// #202: This helper is only reachable through the feature-gated raw linear-memory ABI.
#[cfg_attr(not(feature = "unstable-internals"), allow(dead_code))]
pub fn trusted_read_u128(src: &[u8]) -> u128 {
    debug_assert_eq!(src.len(), 16);
    unsafe { u128::from_le(src.as_ptr().cast::<u128>().read_unaligned()) }
}

#[derive(Debug, Clone, Copy)]
pub struct MemArg {
    pub align: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// #202: Atomic operations are intentionally dormant without the optional threads runtime.
#[cfg_attr(not(feature = "threads"), allow(dead_code))]
pub(crate) enum AtomicRmwOp {
    Add,
    Sub,
    And,
    Or,
    Xor,
    Xchg,
}

impl AtomicRmwOp {
    #[inline(always)]
    fn apply_u8(self, old: u8, value: u8) -> u8 {
        match self {
            Self::Add => old.wrapping_add(value),
            Self::Sub => old.wrapping_sub(value),
            Self::And => old & value,
            Self::Or => old | value,
            Self::Xor => old ^ value,
            Self::Xchg => value,
        }
    }

    #[inline(always)]
    fn apply_u16(self, old: u16, value: u16) -> u16 {
        match self {
            Self::Add => old.wrapping_add(value),
            Self::Sub => old.wrapping_sub(value),
            Self::And => old & value,
            Self::Or => old | value,
            Self::Xor => old ^ value,
            Self::Xchg => value,
        }
    }

    #[inline(always)]
    fn apply_u32(self, old: u32, value: u32) -> u32 {
        match self {
            Self::Add => old.wrapping_add(value),
            Self::Sub => old.wrapping_sub(value),
            Self::And => old & value,
            Self::Or => old | value,
            Self::Xor => old ^ value,
            Self::Xchg => value,
        }
    }

    #[inline(always)]
    fn apply_u64(self, old: u64, value: u64) -> u64 {
        match self {
            Self::Add => old.wrapping_add(value),
            Self::Sub => old.wrapping_sub(value),
            Self::And => old & value,
            Self::Or => old | value,
            Self::Xor => old ^ value,
            Self::Xchg => value,
        }
    }
}

#[cfg(feature = "threads")]
#[derive(Debug)]
pub struct SharedWaitRegistration {
    address: usize,
    waiter: Arc<SharedWaiter>,
}

#[cfg(feature = "threads")]
impl SharedWaitRegistration {
    // #202: Keep the internal queue address for wait-removal bookkeeping.
    #[allow(dead_code)]
    pub(crate) fn address(&self) -> usize {
        self.address
    }

    /// Waits for a notify on this registration and reports the
    /// `memory.atomic.wait` result code: 0 when notified, 2 when the
    /// timeout elapsed.
    ///
    /// A negative `timeout_ns` waits indefinitely. A non-negative timeout
    /// is armed on a detached timer thread rather than an executor timer,
    /// so this future can be driven by any executor — with or without a
    /// Tokio runtime — and never panics for lack of a reactor (#151). The
    /// timer thread is released as soon as the wait resolves (#270).
    pub async fn wait_result(self, shared: Arc<SharedMemoryObject>, timeout_ns: i64) -> i32 {
        match self
            .wait_result_interruptible(shared, timeout_ns, None)
            .await
        {
            VMResult::Success(code) => code,
            // With no interrupt source attached the wait can only resolve
            // as notified or timed out.
            _ => unreachable!("wait without an interrupt source cannot be interrupted"),
        }
    }

    /// Waits for a notify on this registration like [`Self::wait_result`],
    /// but also races the wait against a metering interrupt (#177).
    ///
    /// When `interrupt` is `Some` and [`MeteringHandle::interrupt`] fires
    /// before a notify or the timeout resolves the wait, the wait is
    /// removed from the shared wait queue and this returns
    /// [`VMResult::Cancelled`] — the same distinguishable outcome as a
    /// guest stopped at a metering checkpoint — instead of a wait result
    /// code. The three wake sources race and the first one to claim the
    /// waiter's state wins; a notify or timeout that claims it first keeps
    /// its usual result code even if the interrupt fires concurrently.
    ///
    /// The wait is therefore no longer resolved from a single wake source:
    /// it additionally awaits the metering state's broadcast `Notify`.
    /// That is the deliberate trade for making a parked guest evictable;
    /// the claim itself remains a single compare-exchange on the waiter
    /// state, exactly like the pre-existing notify/timeout race.
    pub async fn wait_result_interruptible(
        self,
        shared: Arc<SharedMemoryObject>,
        timeout_ns: i64,
        interrupt: Option<MeteringHandle>,
    ) -> VMResult<i32> {
        if timeout_ns >= 0 && self.waiter.is_waiting() {
            // #151: tokio::time::sleep panics ("no reactor running") when
            // the embedder drives execution outside a Tokio runtime, and
            // nothing in the driver contract requires one. Mark the waiter
            // as timed out from a detached timer thread and wake it through
            // the same Notify a real notify uses; if a notify wins the
            // race, the mark attempt fails and the thread leaves the wait
            // untouched. The thread's sleep is interruptible, so a notify
            // or interrupt that resolves the wait also releases the
            // thread (#270).
            //
            // #270: a registration that was already resolved before this
            // poll (e.g. notified while queued) gets no timer thread at
            // all; the wait below returns its result immediately.
            let timer_waiter = Arc::clone(&self.waiter);
            std::thread::spawn(move || {
                run_wait_timer(timer_waiter, Duration::from_nanos(timeout_ns as u64));
            });
        }

        let interrupt_won = match &interrupt {
            Some(handle) => {
                let wait_fut = self.waiter.wait();
                futures::pin_mut!(wait_fut);
                let interrupt_fut = handle.wait_for_interrupt();
                futures::pin_mut!(interrupt_fut);
                match futures::future::select(wait_fut, interrupt_fut).await {
                    futures::future::Either::Left(_) => false,
                    futures::future::Either::Right(_) => true,
                }
            }
            None => {
                self.waiter.wait().await;
                false
            }
        };

        if interrupt_won {
            if self.waiter.try_mark_interrupted() {
                shared.remove_waiter(self.address, self.waiter.id());
                return VMResult::Cancelled;
            }
            // A notify or the timeout claimed the waiter first; its wake
            // is already delivered or in flight, so this resolves at once
            // and the earlier claim keeps its result code.
            self.waiter.wait().await;
        }

        if self.waiter.is_timed_out() {
            shared.remove_waiter(self.address, self.waiter.id());
            VMResult::Success(2)
        } else {
            VMResult::Success(0)
        }
    }
}

/// Number of wait-timer threads currently alive. Test-only
/// instrumentation for #270: the regression test uses it to show that
/// timer threads are released when their wait resolves instead of
/// sleeping on until the original deadline.
#[cfg(all(test, feature = "threads"))]
static LIVE_WAIT_TIMER_THREADS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(all(test, feature = "threads"))]
fn live_wait_timer_threads() -> usize {
    LIVE_WAIT_TIMER_THREADS.load(Ordering::SeqCst)
}

/// Body of the detached timer thread armed by
/// [`SharedWaitRegistration::wait_result_interruptible`] for a finite
/// timeout (#151). Sleeps until the deadline, then tries to claim the
/// waiter as timed out and wakes it through the same `Notify` a real
/// notify uses; if a notify won the race, the claim fails and the wait
/// is left untouched.
///
/// The sleep is interruptible (#270): the thread waits on the waiter's
/// release condvar, which every resolution of the wait (notify,
/// interrupt) signals, and re-checks the waiter state on each wake. A
/// wait that resolves early therefore releases its timer thread at
/// once instead of leaving it asleep until the original deadline.
#[cfg(feature = "threads")]
fn run_wait_timer(waiter: Arc<SharedWaiter>, timeout: Duration) {
    #[cfg(test)]
    LIVE_WAIT_TIMER_THREADS.fetch_add(1, Ordering::SeqCst);
    // A timeout near i64::MAX nanoseconds (~292 years) can overflow
    // Instant arithmetic; in that case there is no representable
    // deadline and the thread simply re-checks the state in bounded
    // slices until the wait resolves.
    let deadline = Instant::now().checked_add(timeout);
    {
        let (lock, cvar) = &waiter.timer_release;
        let mut guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        while waiter.is_waiting() {
            let remaining = match deadline {
                Some(deadline) => {
                    let now = Instant::now();
                    if now >= deadline {
                        break;
                    }
                    deadline - now
                }
                None => Duration::from_secs(3600),
            };
            let (next_guard, _timeout_result) = cvar
                .wait_timeout(guard, remaining)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard = next_guard;
        }
    }
    if waiter.try_mark_timed_out() {
        waiter.notify.notify_one();
    }
    #[cfg(test)]
    LIVE_WAIT_TIMER_THREADS.fetch_sub(1, Ordering::SeqCst);
}

#[cfg(feature = "threads")]
#[derive(Debug)]
pub(crate) enum AtomicWaitResult {
    NotEqual,
    Pending(SharedWaitRegistration),
}

#[cfg(feature = "threads")]
#[derive(Debug)]
struct SharedWaiter {
    id: u64,
    state: AtomicU8,
    notify: Notify,
    /// #270: release latch for the detached timer thread armed for a
    /// finite timeout. The timer sleeps on this condvar instead of in an
    /// uninterruptible `thread::sleep`, and every transition out of
    /// `WAITING` signals it, so the thread exits as soon as the wait
    /// resolves instead of sleeping on until the original deadline.
    timer_release: (std::sync::Mutex<()>, std::sync::Condvar),
}

#[cfg(feature = "threads")]
impl SharedWaiter {
    const WAITING: u8 = 0;
    const NOTIFIED: u8 = 1;
    const TIMED_OUT: u8 = 2;
    const INTERRUPTED: u8 = 3;

    fn new(id: u64) -> Self {
        Self {
            id,
            state: AtomicU8::new(Self::WAITING),
            notify: Notify::new(),
            timer_release: (std::sync::Mutex::new(()), std::sync::Condvar::new()),
        }
    }

    fn id(&self) -> u64 {
        self.id
    }

    async fn wait(&self) {
        if self.state.load(Ordering::Acquire) == Self::NOTIFIED {
            return;
        }
        self.notify.notified().await;
    }

    fn try_mark_notified(&self) -> bool {
        let claimed = self
            .state
            .compare_exchange(
                Self::WAITING,
                Self::NOTIFIED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok();
        if claimed {
            self.release_timer();
        }
        claimed
    }

    /// #270: wakes the detached timer thread (if one is armed) so it can
    /// observe that the wait has resolved and exit, instead of sleeping
    /// on until the original deadline. The lock is taken before
    /// signalling so the signal cannot slip between the timer's state
    /// check and its condvar wait: the timer re-checks the state while
    /// holding the same lock, so either it sees the resolved state
    /// before waiting, or it is already waiting and is woken here.
    fn release_timer(&self) {
        let (lock, cvar) = &self.timer_release;
        let _guard = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        cvar.notify_all();
    }

    fn try_mark_timed_out(&self) -> bool {
        self.state
            .compare_exchange(
                Self::WAITING,
                Self::TIMED_OUT,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    fn try_mark_interrupted(&self) -> bool {
        let claimed = self
            .state
            .compare_exchange(
                Self::WAITING,
                Self::INTERRUPTED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok();
        if claimed {
            self.release_timer();
        }
        claimed
    }

    fn is_waiting(&self) -> bool {
        self.state.load(Ordering::Acquire) == Self::WAITING
    }

    fn is_timed_out(&self) -> bool {
        self.state.load(Ordering::Acquire) == Self::TIMED_OUT
    }
}

/// Exact on every target: page counts are `u32` and `PAGE_SIZE` is 2^16, so
/// their product is at most 2^48.
pub(crate) const fn reservation_bytes_u64(max_pages: u32) -> u64 {
    (max_pages as u64) * (PAGE_SIZE as u64)
}

pub(crate) fn reservation_bytes(max_pages: u32) -> Result<usize, MemoryInitError> {
    let bytes = reservation_bytes_u64(max_pages).max(PAGE_SIZE as u64);
    usize::try_from(bytes).map_err(|_| MemoryInitError::ReservationTooLarge { pages: max_pages })
}

fn committed_bytes_for_pages(page_count: u32) -> Result<usize, MemoryInitError> {
    usize::try_from(reservation_bytes_u64(page_count))
        .map_err(|_| MemoryInitError::ReservationTooLarge { pages: page_count })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemoryMappingOperation {
    Mmap,
    Mprotect,
}

impl fmt::Display for MemoryMappingOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mmap => f.write_str("mmap"),
            Self::Mprotect => f.write_str("mprotect"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MemoryInitError {
    InvalidPageBounds {
        page_count: u32,
        max_page_size: u32,
    },
    ReservationTooLarge {
        pages: u32,
    },
    MappingFailed {
        operation: MemoryMappingOperation,
        bytes: usize,
        errno: Option<i32>,
    },
    InvalidCommitLength {
        requested_bytes: usize,
        committed_bytes: usize,
        reserved_bytes: usize,
    },
}

impl fmt::Display for MemoryInitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPageBounds {
                page_count,
                max_page_size,
            } => write!(
                f,
                "invalid memory bounds: page_count ({page_count}) must be <= max_page_size ({max_page_size})"
            ),
            Self::ReservationTooLarge { pages } => write!(
                f,
                "memory reservation for {pages} pages does not fit in the host address space"
            ),
            Self::MappingFailed {
                operation,
                bytes,
                errno,
            } => match errno {
                Some(errno) => write!(
                    f,
                    "memory {operation} failed for {bytes} bytes (errno {errno})"
                ),
                None => write!(f, "memory {operation} failed for {bytes} bytes"),
            },
            Self::InvalidCommitLength {
                requested_bytes,
                committed_bytes,
                reserved_bytes,
            } => write!(
                f,
                "invalid memory commit length {requested_bytes}; committed={committed_bytes}, reserved={reserved_bytes}"
            ),
        }
    }
}

impl std::error::Error for MemoryInitError {}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TestMemoryMappingFailure {
    Mmap,
    Mprotect,
}

#[cfg(test)]
thread_local! {
    static TEST_MEMORY_MAPPING_FAILURE: Cell<Option<TestMemoryMappingFailure>> = const { Cell::new(None) };
}

#[cfg(test)]
pub(crate) struct TestMemoryMappingFailureGuard {
    previous: Option<TestMemoryMappingFailure>,
}

#[cfg(test)]
pub(crate) fn fail_next_memory_mapping(
    failure: TestMemoryMappingFailure,
) -> TestMemoryMappingFailureGuard {
    let previous = TEST_MEMORY_MAPPING_FAILURE.with(|state| state.replace(Some(failure)));
    TestMemoryMappingFailureGuard { previous }
}

#[cfg(test)]
impl Drop for TestMemoryMappingFailureGuard {
    fn drop(&mut self) {
        TEST_MEMORY_MAPPING_FAILURE.with(|state| state.set(self.previous));
    }
}

#[cfg(test)]
fn take_test_memory_mapping_failure(failure: TestMemoryMappingFailure) -> bool {
    TEST_MEMORY_MAPPING_FAILURE.with(|state| {
        if state.get() == Some(failure) {
            state.set(None);
            true
        } else {
            false
        }
    })
}

fn mmap_region(len: usize, flags: libc::c_int) -> Result<NonNull<u8>, MemoryInitError> {
    let ptr = unsafe { libc::mmap(std::ptr::null_mut(), len, libc::PROT_NONE, flags, -1, 0) };
    if ptr == libc::MAP_FAILED {
        return Err(MemoryInitError::MappingFailed {
            operation: MemoryMappingOperation::Mmap,
            bytes: len,
            errno: std::io::Error::last_os_error().raw_os_error(),
        });
    }
    if ptr.is_null() {
        let _ = unsafe { libc::munmap(ptr, len) };
        return Err(MemoryInitError::MappingFailed {
            operation: MemoryMappingOperation::Mmap,
            bytes: len,
            errno: None,
        });
    }
    Ok(unsafe { NonNull::new_unchecked(ptr.cast::<u8>()) })
}

fn mprotect_delta(ptr: *mut libc::c_void, len: usize) -> Result<(), Option<i32>> {
    if unsafe { libc::mprotect(ptr, len, libc::PROT_READ | libc::PROT_WRITE) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().raw_os_error())
    }
}

#[derive(Debug)]
struct MmapRegion {
    ptr: NonNull<u8>,
    reserved_bytes: usize,
    committed_bytes: usize,
}

impl MmapRegion {
    fn new(len: usize, shared: bool) -> Result<Self, MemoryInitError> {
        let mut flags = libc::MAP_ANON;
        flags |= if shared {
            libc::MAP_SHARED
        } else {
            libc::MAP_PRIVATE
        };
        #[cfg(target_os = "linux")]
        {
            flags |= libc::MAP_NORESERVE;
        }

        #[cfg(test)]
        if take_test_memory_mapping_failure(TestMemoryMappingFailure::Mmap) {
            return Err(MemoryInitError::MappingFailed {
                operation: MemoryMappingOperation::Mmap,
                bytes: len,
                errno: Some(libc::ENOMEM),
            });
        }
        let ptr = mmap_region(len, flags)?;
        Ok(Self {
            ptr,
            reserved_bytes: len,
            committed_bytes: 0,
        })
    }

    fn commit(&mut self, new_len: usize) -> Result<(), MemoryInitError> {
        if new_len == 0 || new_len == self.committed_bytes {
            return Ok(());
        }
        if new_len < self.committed_bytes || new_len > self.reserved_bytes {
            return Err(MemoryInitError::InvalidCommitLength {
                requested_bytes: new_len,
                committed_bytes: self.committed_bytes,
                reserved_bytes: self.reserved_bytes,
            });
        }
        let ptr = unsafe {
            self.ptr
                .as_ptr()
                .add(self.committed_bytes)
                .cast::<libc::c_void>()
        };
        let bytes = new_len - self.committed_bytes;
        #[cfg(test)]
        if take_test_memory_mapping_failure(TestMemoryMappingFailure::Mprotect) {
            return Err(MemoryInitError::MappingFailed {
                operation: MemoryMappingOperation::Mprotect,
                bytes,
                errno: Some(libc::ENOMEM),
            });
        }
        if let Err(errno) = mprotect_delta(ptr, bytes) {
            return Err(MemoryInitError::MappingFailed {
                operation: MemoryMappingOperation::Mprotect,
                bytes,
                errno,
            });
        }
        self.committed_bytes = new_len;
        Ok(())
    }

    fn reserved_bytes(&self) -> usize {
        self.reserved_bytes
    }

    fn committed_bytes(&self) -> usize {
        self.committed_bytes
    }

    fn as_slice(&self, len: usize) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), len) }
    }

    fn as_slice_mut(&mut self, len: usize) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), len) }
    }
}

impl Drop for MmapRegion {
    fn drop(&mut self) {
        let _ = unsafe {
            libc::munmap(
                self.ptr.as_ptr().cast::<libc::c_void>(),
                self.reserved_bytes,
            )
        };
    }
}

// SAFETY: the mapping owns its virtual memory reservation and raw pointer metadata only.
// Cross-thread access is synchronized by the caller (`Store` single-flight or shared-memory mutex).
unsafe impl Send for MmapRegion {}

// #202: Raw memory accessors are exposed only through `unstable-internals`.
#[cfg_attr(not(feature = "unstable-internals"), allow(dead_code))]
fn compute_offset(memarg: MemArg, offset: u32) -> VMResult<usize> {
    VMResult::from_option(checked_memory_offset(memarg.offset, offset), || {
        VMResult::MemoryIndexOutOfRange
    })
}

#[inline(always)]
fn ensure_atomic_alignment(offset: usize, alignment: usize) -> VMResult<()> {
    if atomic_alignment_valid(offset, alignment) {
        VMResult::Success(())
    } else {
        VMResult::UnalignedAtomic
    }
}

pub struct Memory {
    region: MmapRegion,
    current_pages: u32,
    max_pages: u32,
}

#[derive(Debug, Clone, Copy)]
#[cfg(feature = "jit")]
pub(crate) struct MemoryJitLayout {
    pub(crate) region_ptr: usize,
    pub(crate) current_pages: usize,
}

#[cfg(feature = "jit")]
impl MemoryJitLayout {
    pub(crate) fn get() -> Self {
        Self {
            region_ptr: std::mem::offset_of!(Memory, region)
                + std::mem::offset_of!(MmapRegion, ptr),
            current_pages: std::mem::offset_of!(Memory, current_pages),
        }
    }
}

impl fmt::Debug for Memory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Memory")
            .field("current_pages", &self.current_pages)
            .field("max_pages", &self.max_pages)
            .finish()
    }
}

// #202: The raw linear-memory implementation is exposed only through `unstable-internals`.
#[cfg_attr(not(feature = "unstable-internals"), allow(dead_code))]
impl Memory {
    #[inline(always)]
    fn base_ptr(&self) -> *const u8 {
        self.region.ptr.as_ptr()
    }

    pub(crate) fn new(page_count: u32, max_page_size: u32) -> Result<Self, MemoryInitError> {
        Self::new_with_mapping(page_count, max_page_size, false)
    }

    pub(crate) fn new_shared(page_count: u32, max_page_size: u32) -> Result<Self, MemoryInitError> {
        Self::new_with_mapping(page_count, max_page_size, true)
    }

    fn new_with_mapping(
        page_count: u32,
        max_page_size: u32,
        shared: bool,
    ) -> Result<Self, MemoryInitError> {
        if page_count > max_page_size {
            return Err(MemoryInitError::InvalidPageBounds {
                page_count,
                max_page_size,
            });
        }
        let reserved = reservation_bytes(max_page_size)?;
        let committed = committed_bytes_for_pages(page_count)?;
        let mut region = MmapRegion::new(reserved, shared)?;
        region.commit(committed)?;
        debug_assert_eq!(region.committed_bytes(), committed);
        debug_assert_eq!(region.reserved_bytes(), reserved);
        Ok(Self {
            region,
            current_pages: page_count,
            max_pages: max_page_size,
        })
    }

    pub fn page_size(&self) -> u32 {
        self.current_pages
    }

    pub fn data_size(&self) -> usize {
        self.region.committed_bytes()
    }

    #[cfg(test)]
    pub(crate) fn reserved_bytes(&self) -> usize {
        self.region.reserved_bytes()
    }

    #[cfg(test)]
    pub(crate) fn committed_bytes(&self) -> usize {
        self.region.committed_bytes()
    }

    #[inline(always)]
    pub(crate) fn data_ptr(&self) -> *const u8 {
        self.base_ptr()
    }

    #[inline(always)]
    pub(crate) fn data_mut_ptr(&mut self) -> *mut u8 {
        self.region.ptr.as_ptr()
    }

    fn slice(&self) -> &[u8] {
        self.region.as_slice(self.data_size())
    }

    fn slice_mut(&mut self) -> &mut [u8] {
        self.region.as_slice_mut(self.data_size())
    }

    pub fn get<I: SliceIndex<[u8]>>(&self, range: I) -> Option<&I::Output> {
        self.slice().get(range)
    }

    pub fn get_mut<I: SliceIndex<[u8]>>(&mut self, range: I) -> Option<&mut I::Output> {
        self.slice_mut().get_mut(range)
    }

    #[inline(always)]
    pub fn write_bytes(&mut self, offset: usize, bytes: &[u8]) -> VMResult<()> {
        let end = vm_try!(VMResult::from_option(
            offset.checked_add(bytes.len()),
            || VMResult::MemoryIndexOutOfRange
        ));
        let target = vm_try!(VMResult::from_option(
            self.slice_mut().get_mut(offset..end),
            || { VMResult::MemoryIndexOutOfRange }
        ));
        trusted_copy_from_slice(target, bytes);
        VMResult::Success(())
    }

    #[inline(always)]
    pub fn write_u32_at(&mut self, offset: usize, value: u32) -> VMResult<()> {
        let last = vm_try!(VMResult::from_option(offset.checked_add(4), || {
            VMResult::MemoryIndexOutOfRange
        }));
        if last > self.data_size() {
            return VMResult::MemoryIndexOutOfRange;
        }
        unsafe {
            self.region
                .ptr
                .as_ptr()
                .add(offset)
                .cast::<u32>()
                .write_unaligned(value.to_le());
        }
        VMResult::Success(())
    }

    #[inline(always)]
    pub fn push_to_stack<const N: usize>(&self, stack: &mut Stack, offset: usize) -> VMResult<()> {
        let last = vm_try!(VMResult::from_option(offset.checked_add(N), || {
            VMResult::MemoryIndexOutOfRange
        }));
        if last > self.data_size() {
            return VMResult::MemoryIndexOutOfRange;
        }
        unsafe { stack.push_copy_from_ptr::<N>(self.base_ptr().add(offset)) }
    }

    pub fn read_u8_array<const N: usize>(&self, offset: usize) -> VMResult<[u8; N]> {
        let mut arr = [0u8; N];
        let last = vm_try!(VMResult::from_option(offset.checked_add(N), || {
            VMResult::MemoryIndexOutOfRange
        }));
        trusted_copy_from_slice(
            &mut arr[..],
            vm_try!(VMResult::from_option(
                self.slice().get(offset..last),
                || { VMResult::MemoryIndexOutOfRange }
            )),
        );
        VMResult::Success(arr)
    }

    #[inline(always)]
    pub fn read_u8_at(&self, offset: usize) -> VMResult<u8> {
        if offset >= self.data_size() {
            return VMResult::MemoryIndexOutOfRange;
        }
        VMResult::Success(unsafe { *self.base_ptr().add(offset) })
    }

    #[inline(always)]
    pub fn read_i8_at(&self, offset: usize) -> VMResult<i8> {
        VMResult::Success(vm_try!(self.read_u8_at(offset)) as i8)
    }

    #[inline(always)]
    pub fn read_u16_at(&self, offset: usize) -> VMResult<u16> {
        let last = vm_try!(VMResult::from_option(offset.checked_add(2), || {
            VMResult::MemoryIndexOutOfRange
        }));
        if last > self.data_size() {
            return VMResult::MemoryIndexOutOfRange;
        }
        VMResult::Success(u16::from_le(unsafe {
            self.base_ptr().add(offset).cast::<u16>().read_unaligned()
        }))
    }

    #[inline(always)]
    pub fn read_i16_at(&self, offset: usize) -> VMResult<i16> {
        VMResult::Success(vm_try!(self.read_u16_at(offset)) as i16)
    }

    #[inline(always)]
    pub fn read_u32_at(&self, offset: usize) -> VMResult<u32> {
        let last = vm_try!(VMResult::from_option(offset.checked_add(4), || {
            VMResult::MemoryIndexOutOfRange
        }));
        if last > self.data_size() {
            return VMResult::MemoryIndexOutOfRange;
        }
        VMResult::Success(u32::from_le(unsafe {
            self.base_ptr().add(offset).cast::<u32>().read_unaligned()
        }))
    }

    #[inline(always)]
    pub fn read_i32_at(&self, offset: usize) -> VMResult<i32> {
        VMResult::Success(vm_try!(self.read_u32_at(offset)) as i32)
    }

    #[inline(always)]
    pub fn read_u64_at(&self, offset: usize) -> VMResult<u64> {
        let last = vm_try!(VMResult::from_option(offset.checked_add(8), || {
            VMResult::MemoryIndexOutOfRange
        }));
        if last > self.data_size() {
            return VMResult::MemoryIndexOutOfRange;
        }
        VMResult::Success(u64::from_le(unsafe {
            self.base_ptr().add(offset).cast::<u64>().read_unaligned()
        }))
    }

    #[inline(always)]
    pub fn read_i64_at(&self, offset: usize) -> VMResult<i64> {
        VMResult::Success(vm_try!(self.read_u64_at(offset)) as i64)
    }

    #[inline(always)]
    pub fn read_f32_at(&self, offset: usize) -> VMResult<f32> {
        VMResult::Success(f32::from_bits(vm_try!(self.read_u32_at(offset))))
    }

    #[inline(always)]
    pub fn read_f64_at(&self, offset: usize) -> VMResult<f64> {
        VMResult::Success(f64::from_bits(vm_try!(self.read_u64_at(offset))))
    }

    fn write_slice(&mut self, memarg: MemArg, offset: u32, value: &[u8]) -> VMResult<()> {
        let offset = vm_try!(compute_offset(memarg, offset));
        let n = value.len();
        let last = vm_try!(VMResult::from_option(offset.checked_add(n), || {
            VMResult::MemoryIndexOutOfRange
        }));
        let dst = vm_try!(VMResult::from_option(
            self.slice_mut().get_mut(offset..last),
            || VMResult::MemoryIndexOutOfRange
        ));
        trusted_copy_from_slice(dst, value);
        VMResult::Success(())
    }

    pub fn write_f32(&mut self, memarg: MemArg, offset: u32, value: f32) -> VMResult<()> {
        self.write_slice(memarg, offset, &value.to_le_bytes())
    }

    pub fn write_f64(&mut self, memarg: MemArg, offset: u32, value: f64) -> VMResult<()> {
        self.write_slice(memarg, offset, &value.to_le_bytes())
    }

    pub fn write_u32(&mut self, memarg: MemArg, offset: u32, value: u32) -> VMResult<()> {
        self.write_slice(memarg, offset, &value.to_le_bytes())
    }

    pub fn write_u64(&mut self, memarg: MemArg, offset: u32, value: u64) -> VMResult<()> {
        self.write_slice(memarg, offset, &value.to_le_bytes())
    }

    pub fn write_u128(&mut self, memarg: MemArg, offset: u32, value: u128) -> VMResult<()> {
        self.write_slice(memarg, offset, &value.to_le_bytes())
    }

    pub fn write_u8(&mut self, memarg: MemArg, offset: u32, value: u8) -> VMResult<()> {
        *vm_try!(VMResult::from_option(
            self.slice_mut()
                .get_mut(vm_try!(compute_offset(memarg, offset))),
            || VMResult::MemoryIndexOutOfRange
        )) = value;
        VMResult::Success(())
    }

    pub fn write_u16(&mut self, memarg: MemArg, offset: u32, value: u16) -> VMResult<()> {
        self.write_slice(memarg, offset, &value.to_le_bytes())
    }

    pub fn read_i32(&self, memarg: MemArg, offset: u32) -> VMResult<i32> {
        VMResult::Success(i32::from_le_bytes(vm_try!(
            self.read_u8_array::<4>(vm_try!(compute_offset(memarg, offset)))
        )))
    }

    pub fn read_i64(&self, memarg: MemArg, offset: u32) -> VMResult<i64> {
        VMResult::Success(i64::from_le_bytes(vm_try!(
            self.read_u8_array::<8>(vm_try!(compute_offset(memarg, offset)))
        )))
    }

    pub fn read_u32(&self, memarg: MemArg, offset: u32) -> VMResult<u32> {
        VMResult::Success(u32::from_le_bytes(vm_try!(
            self.read_u8_array::<4>(vm_try!(compute_offset(memarg, offset)))
        )))
    }

    pub fn read_u64(&self, memarg: MemArg, offset: u32) -> VMResult<u64> {
        VMResult::Success(u64::from_le_bytes(vm_try!(
            self.read_u8_array::<8>(vm_try!(compute_offset(memarg, offset)))
        )))
    }

    pub fn read_u128(&self, memarg: MemArg, offset: u32) -> VMResult<u128> {
        let start = vm_try!(compute_offset(memarg, offset));
        let last = vm_try!(VMResult::from_option(start.checked_add(16), || {
            VMResult::MemoryIndexOutOfRange
        }));
        let bytes = vm_try!(VMResult::from_option(self.slice().get(start..last), || {
            VMResult::MemoryIndexOutOfRange
        }));
        VMResult::Success(trusted_read_u128(bytes))
    }

    pub fn read_f32(&self, memarg: MemArg, offset: u32) -> VMResult<f32> {
        VMResult::Success(f32::from_le_bytes(vm_try!(
            self.read_u8_array::<4>(vm_try!(compute_offset(memarg, offset)))
        )))
    }

    pub fn read_f64(&self, memarg: MemArg, offset: u32) -> VMResult<f64> {
        VMResult::Success(f64::from_le_bytes(vm_try!(
            self.read_u8_array::<8>(vm_try!(compute_offset(memarg, offset)))
        )))
    }

    pub fn read_u8(&self, memarg: MemArg, offset: u32) -> VMResult<u8> {
        VMResult::Success(
            vm_try!(self.read_u8_array::<1>(vm_try!(compute_offset(memarg, offset))))[0],
        )
    }

    pub fn read_i8(&self, memarg: MemArg, offset: u32) -> VMResult<i8> {
        VMResult::Success(
            vm_try!(self.read_u8_array::<1>(vm_try!(compute_offset(memarg, offset))))[0] as i8,
        )
    }

    pub fn read_i16(&self, memarg: MemArg, offset: u32) -> VMResult<i16> {
        VMResult::Success(i16::from_le_bytes(vm_try!(
            self.read_u8_array::<2>(vm_try!(compute_offset(memarg, offset)))
        )))
    }

    pub fn read_u16(&self, memarg: MemArg, offset: u32) -> VMResult<u16> {
        VMResult::Success(u16::from_le_bytes(vm_try!(
            self.read_u8_array::<2>(vm_try!(compute_offset(memarg, offset)))
        )))
    }

    #[inline(always)]
    pub fn atomic_load_u8(&self, offset: usize) -> VMResult<u8> {
        vm_try!(ensure_atomic_alignment(offset, 1));
        self.read_u8_at(offset)
    }

    #[inline(always)]
    pub fn atomic_load_u16(&self, offset: usize) -> VMResult<u16> {
        vm_try!(ensure_atomic_alignment(offset, 2));
        self.read_u16_at(offset)
    }

    #[inline(always)]
    pub fn atomic_load_u32(&self, offset: usize) -> VMResult<u32> {
        vm_try!(ensure_atomic_alignment(offset, 4));
        self.read_u32_at(offset)
    }

    #[inline(always)]
    pub fn atomic_load_u64(&self, offset: usize) -> VMResult<u64> {
        vm_try!(ensure_atomic_alignment(offset, 8));
        self.read_u64_at(offset)
    }

    #[inline(always)]
    pub fn atomic_store_u8(&mut self, offset: usize, value: u8) -> VMResult<()> {
        vm_try!(ensure_atomic_alignment(offset, 1));
        self.write_bytes(offset, &[value])
    }

    #[inline(always)]
    pub fn atomic_store_u16(&mut self, offset: usize, value: u16) -> VMResult<()> {
        vm_try!(ensure_atomic_alignment(offset, 2));
        self.write_bytes(offset, &value.to_le_bytes())
    }

    #[inline(always)]
    pub fn atomic_store_u32(&mut self, offset: usize, value: u32) -> VMResult<()> {
        vm_try!(ensure_atomic_alignment(offset, 4));
        self.write_bytes(offset, &value.to_le_bytes())
    }

    #[inline(always)]
    pub fn atomic_store_u64(&mut self, offset: usize, value: u64) -> VMResult<()> {
        vm_try!(ensure_atomic_alignment(offset, 8));
        self.write_bytes(offset, &value.to_le_bytes())
    }

    #[inline(always)]
    pub(crate) fn atomic_rmw_u8(
        &mut self,
        offset: usize,
        op: AtomicRmwOp,
        value: u8,
    ) -> VMResult<u8> {
        let old = vm_try!(self.atomic_load_u8(offset));
        vm_try!(self.atomic_store_u8(offset, op.apply_u8(old, value)));
        VMResult::Success(old)
    }

    #[inline(always)]
    pub(crate) fn atomic_rmw_u16(
        &mut self,
        offset: usize,
        op: AtomicRmwOp,
        value: u16,
    ) -> VMResult<u16> {
        let old = vm_try!(self.atomic_load_u16(offset));
        vm_try!(self.atomic_store_u16(offset, op.apply_u16(old, value)));
        VMResult::Success(old)
    }

    #[inline(always)]
    pub(crate) fn atomic_rmw_u32(
        &mut self,
        offset: usize,
        op: AtomicRmwOp,
        value: u32,
    ) -> VMResult<u32> {
        let old = vm_try!(self.atomic_load_u32(offset));
        vm_try!(self.atomic_store_u32(offset, op.apply_u32(old, value)));
        VMResult::Success(old)
    }

    #[inline(always)]
    pub(crate) fn atomic_rmw_u64(
        &mut self,
        offset: usize,
        op: AtomicRmwOp,
        value: u64,
    ) -> VMResult<u64> {
        let old = vm_try!(self.atomic_load_u64(offset));
        vm_try!(self.atomic_store_u64(offset, op.apply_u64(old, value)));
        VMResult::Success(old)
    }

    #[inline(always)]
    pub fn atomic_cmpxchg_u8(&mut self, offset: usize, expected: u8, value: u8) -> VMResult<u8> {
        let old = vm_try!(self.atomic_load_u8(offset));
        if old == expected {
            vm_try!(self.atomic_store_u8(offset, value));
        }
        VMResult::Success(old)
    }

    #[inline(always)]
    pub fn atomic_cmpxchg_u16(
        &mut self,
        offset: usize,
        expected: u16,
        value: u16,
    ) -> VMResult<u16> {
        let old = vm_try!(self.atomic_load_u16(offset));
        if old == expected {
            vm_try!(self.atomic_store_u16(offset, value));
        }
        VMResult::Success(old)
    }

    #[inline(always)]
    pub fn atomic_cmpxchg_u32(
        &mut self,
        offset: usize,
        expected: u32,
        value: u32,
    ) -> VMResult<u32> {
        let old = vm_try!(self.atomic_load_u32(offset));
        if old == expected {
            vm_try!(self.atomic_store_u32(offset, value));
        }
        VMResult::Success(old)
    }

    #[inline(always)]
    pub fn atomic_cmpxchg_u64(
        &mut self,
        offset: usize,
        expected: u64,
        value: u64,
    ) -> VMResult<u64> {
        let old = vm_try!(self.atomic_load_u64(offset));
        if old == expected {
            vm_try!(self.atomic_store_u64(offset, value));
        }
        VMResult::Success(old)
    }

    #[inline(always)]
    pub fn atomic_fence(&self) {}

    pub fn grow(&mut self, page_size_delta: u32) -> VMResult<i32> {
        let current_page_size = self.page_size();
        let Some(new_page_size) = current_page_size.checked_add(page_size_delta) else {
            return VMResult::Success(-1);
        };
        if new_page_size > self.max_pages {
            return VMResult::Success(-1);
        }
        let Ok(new_committed_bytes) = committed_bytes_for_pages(new_page_size) else {
            return VMResult::Success(-1);
        };
        if new_committed_bytes > self.region.reserved_bytes() {
            return VMResult::Success(-1);
        }
        if self.region.commit(new_committed_bytes).is_err() {
            return VMResult::Success(-1);
        }
        self.current_pages = new_page_size;
        debug_assert_eq!(self.region.committed_bytes(), new_committed_bytes);
        VMResult::Success(current_page_size as i32)
    }

    pub fn fill(&mut self, ptr: u32, len: u32, data: u32) -> VMResult<()> {
        let last = vm_try!(VMResult::from_option(ptr.checked_add(len), || {
            VMResult::MemoryIndexOutOfRange
        }));
        let slice = vm_try!(VMResult::from_option(
            self.slice_mut().get_mut(ptr as usize..last as usize),
            || { VMResult::MemoryIndexOutOfRange }
        ));
        trusted_fill_slice(slice, data as u8);
        VMResult::Success(())
    }

    pub fn copy(&mut self, dst: u32, src: u32, len: u32) -> VMResult<()> {
        let src_last = vm_try!(VMResult::from_option(src.checked_add(len), || {
            VMResult::MemoryIndexOutOfRange
        })) as usize;
        if src_last > self.data_size() {
            return VMResult::MemoryIndexOutOfRange;
        }
        let dst_last = vm_try!(VMResult::from_option(dst.checked_add(len), || {
            VMResult::MemoryIndexOutOfRange
        })) as usize;
        if dst_last > self.data_size() {
            return VMResult::MemoryIndexOutOfRange;
        }
        trusted_copy_within(self.slice_mut(), src as usize, src_last, dst as usize);
        VMResult::Success(())
    }
}

#[derive(Debug)]
pub(crate) struct LocalMemoryObject {
    memory: Memory,
}

impl LocalMemoryObject {
    pub(crate) fn new(page_count: u32, max_page_size: u32) -> Result<Self, MemoryInitError> {
        Ok(Self {
            memory: Memory::new(page_count, max_page_size)?,
        })
    }

    pub fn memory(&self) -> &Memory {
        &self.memory
    }

    pub fn memory_mut(&mut self) -> &mut Memory {
        &mut self.memory
    }

    pub fn page_size(&self) -> u32 {
        self.memory.page_size()
    }

    #[inline(always)]
    pub fn read_u8_array<const N: usize>(&self, offset: usize) -> VMResult<[u8; N]> {
        self.memory.read_u8_array::<N>(offset)
    }

    #[inline(always)]
    pub fn push_to_stack<const N: usize>(&self, stack: &mut Stack, offset: usize) -> VMResult<()> {
        self.memory.push_to_stack::<N>(stack, offset)
    }

    #[inline(always)]
    pub fn read_u8_at(&self, offset: usize) -> VMResult<u8> {
        self.memory.read_u8_at(offset)
    }

    #[inline(always)]
    pub fn read_i8_at(&self, offset: usize) -> VMResult<i8> {
        self.memory.read_i8_at(offset)
    }

    #[inline(always)]
    pub fn read_u16_at(&self, offset: usize) -> VMResult<u16> {
        self.memory.read_u16_at(offset)
    }

    #[inline(always)]
    pub fn read_i16_at(&self, offset: usize) -> VMResult<i16> {
        self.memory.read_i16_at(offset)
    }

    #[inline(always)]
    pub fn read_u32_at(&self, offset: usize) -> VMResult<u32> {
        self.memory.read_u32_at(offset)
    }

    #[inline(always)]
    pub fn read_i32_at(&self, offset: usize) -> VMResult<i32> {
        self.memory.read_i32_at(offset)
    }

    #[inline(always)]
    pub fn read_u64_at(&self, offset: usize) -> VMResult<u64> {
        self.memory.read_u64_at(offset)
    }

    #[inline(always)]
    pub fn read_i64_at(&self, offset: usize) -> VMResult<i64> {
        self.memory.read_i64_at(offset)
    }

    #[inline(always)]
    pub fn read_f32_at(&self, offset: usize) -> VMResult<f32> {
        self.memory.read_f32_at(offset)
    }

    #[inline(always)]
    pub fn read_f64_at(&self, offset: usize) -> VMResult<f64> {
        self.memory.read_f64_at(offset)
    }

    #[inline(always)]
    pub fn write_bytes(&mut self, offset: usize, bytes: &[u8]) -> VMResult<()> {
        self.memory.write_bytes(offset, bytes)
    }

    #[inline(always)]
    pub fn atomic_load_u8(&self, offset: usize) -> VMResult<u8> {
        self.memory.atomic_load_u8(offset)
    }

    #[inline(always)]
    pub fn atomic_load_u16(&self, offset: usize) -> VMResult<u16> {
        self.memory.atomic_load_u16(offset)
    }

    #[inline(always)]
    pub fn atomic_load_u32(&self, offset: usize) -> VMResult<u32> {
        self.memory.atomic_load_u32(offset)
    }

    #[inline(always)]
    pub fn atomic_load_u64(&self, offset: usize) -> VMResult<u64> {
        self.memory.atomic_load_u64(offset)
    }

    #[inline(always)]
    pub fn atomic_store_u8(&mut self, offset: usize, value: u8) -> VMResult<()> {
        self.memory.atomic_store_u8(offset, value)
    }

    #[inline(always)]
    pub fn atomic_store_u16(&mut self, offset: usize, value: u16) -> VMResult<()> {
        self.memory.atomic_store_u16(offset, value)
    }

    #[inline(always)]
    pub fn atomic_store_u32(&mut self, offset: usize, value: u32) -> VMResult<()> {
        self.memory.atomic_store_u32(offset, value)
    }

    #[inline(always)]
    pub fn atomic_store_u64(&mut self, offset: usize, value: u64) -> VMResult<()> {
        self.memory.atomic_store_u64(offset, value)
    }

    #[inline(always)]
    pub(crate) fn atomic_rmw_u8(
        &mut self,
        offset: usize,
        op: AtomicRmwOp,
        value: u8,
    ) -> VMResult<u8> {
        self.memory.atomic_rmw_u8(offset, op, value)
    }

    #[inline(always)]
    pub(crate) fn atomic_rmw_u16(
        &mut self,
        offset: usize,
        op: AtomicRmwOp,
        value: u16,
    ) -> VMResult<u16> {
        self.memory.atomic_rmw_u16(offset, op, value)
    }

    #[inline(always)]
    pub(crate) fn atomic_rmw_u32(
        &mut self,
        offset: usize,
        op: AtomicRmwOp,
        value: u32,
    ) -> VMResult<u32> {
        self.memory.atomic_rmw_u32(offset, op, value)
    }

    #[inline(always)]
    pub(crate) fn atomic_rmw_u64(
        &mut self,
        offset: usize,
        op: AtomicRmwOp,
        value: u64,
    ) -> VMResult<u64> {
        self.memory.atomic_rmw_u64(offset, op, value)
    }

    #[inline(always)]
    pub fn atomic_cmpxchg_u8(&mut self, offset: usize, expected: u8, value: u8) -> VMResult<u8> {
        self.memory.atomic_cmpxchg_u8(offset, expected, value)
    }

    #[inline(always)]
    pub fn atomic_cmpxchg_u16(
        &mut self,
        offset: usize,
        expected: u16,
        value: u16,
    ) -> VMResult<u16> {
        self.memory.atomic_cmpxchg_u16(offset, expected, value)
    }

    #[inline(always)]
    pub fn atomic_cmpxchg_u32(
        &mut self,
        offset: usize,
        expected: u32,
        value: u32,
    ) -> VMResult<u32> {
        self.memory.atomic_cmpxchg_u32(offset, expected, value)
    }

    #[inline(always)]
    pub fn atomic_cmpxchg_u64(
        &mut self,
        offset: usize,
        expected: u64,
        value: u64,
    ) -> VMResult<u64> {
        self.memory.atomic_cmpxchg_u64(offset, expected, value)
    }

    #[inline(always)]
    pub fn atomic_fence(&self) {
        self.memory.atomic_fence();
    }

    #[inline(always)]
    pub fn grow(&mut self, page_size_delta: u32) -> VMResult<i32> {
        self.memory.grow(page_size_delta)
    }

    #[inline(always)]
    pub fn copy(&mut self, dst: u32, src: u32, len: u32) -> VMResult<()> {
        self.memory.copy(dst, src, len)
    }

    #[inline(always)]
    pub fn fill(&mut self, ptr: u32, len: u32, data: u32) -> VMResult<()> {
        self.memory.fill(ptr, len, data)
    }
}

#[derive(Debug)]
struct SharedMemoryState {
    memory: Memory,
    #[cfg(feature = "threads")]
    wait_queues: HashMap<usize, VecDeque<Arc<SharedWaiter>>>,
    #[cfg(feature = "threads")]
    next_waiter_id: u64,
}

#[derive(Debug)]
pub struct SharedMemoryObject {
    state: Mutex<SharedMemoryState>,
}

impl SharedMemoryObject {
    pub(crate) fn new(page_count: u32, max_page_size: u32) -> Result<Arc<Self>, MemoryInitError> {
        Ok(Arc::new(Self {
            state: Mutex::new(SharedMemoryState {
                memory: Memory::new_shared(page_count, max_page_size)?,
                #[cfg(feature = "threads")]
                wait_queues: HashMap::new(),
                #[cfg(feature = "threads")]
                next_waiter_id: 1,
            }),
        }))
    }

    pub fn page_size(&self) -> u32 {
        self.state.lock().memory.page_size()
    }

    #[inline(always)]
    pub fn read_u8_array<const N: usize>(&self, offset: usize) -> VMResult<[u8; N]> {
        self.state.lock().memory.read_u8_array::<N>(offset)
    }

    #[inline(always)]
    pub fn push_to_stack<const N: usize>(&self, stack: &mut Stack, offset: usize) -> VMResult<()> {
        self.state.lock().memory.push_to_stack::<N>(stack, offset)
    }

    #[inline(always)]
    pub fn read_u8_at(&self, offset: usize) -> VMResult<u8> {
        self.state.lock().memory.read_u8_at(offset)
    }

    #[inline(always)]
    pub fn read_i8_at(&self, offset: usize) -> VMResult<i8> {
        self.state.lock().memory.read_i8_at(offset)
    }

    #[inline(always)]
    pub fn read_u16_at(&self, offset: usize) -> VMResult<u16> {
        self.state.lock().memory.read_u16_at(offset)
    }

    #[inline(always)]
    pub fn read_i16_at(&self, offset: usize) -> VMResult<i16> {
        self.state.lock().memory.read_i16_at(offset)
    }

    #[inline(always)]
    pub fn read_u32_at(&self, offset: usize) -> VMResult<u32> {
        self.state.lock().memory.read_u32_at(offset)
    }

    #[inline(always)]
    pub fn read_i32_at(&self, offset: usize) -> VMResult<i32> {
        self.state.lock().memory.read_i32_at(offset)
    }

    #[inline(always)]
    pub fn read_u64_at(&self, offset: usize) -> VMResult<u64> {
        self.state.lock().memory.read_u64_at(offset)
    }

    #[inline(always)]
    pub fn read_i64_at(&self, offset: usize) -> VMResult<i64> {
        self.state.lock().memory.read_i64_at(offset)
    }

    #[inline(always)]
    pub fn read_f32_at(&self, offset: usize) -> VMResult<f32> {
        self.state.lock().memory.read_f32_at(offset)
    }

    #[inline(always)]
    pub fn read_f64_at(&self, offset: usize) -> VMResult<f64> {
        self.state.lock().memory.read_f64_at(offset)
    }

    #[inline(always)]
    pub fn write_bytes(&self, offset: usize, bytes: &[u8]) -> VMResult<()> {
        self.state.lock().memory.write_bytes(offset, bytes)
    }

    #[inline(always)]
    pub fn atomic_load_u8(&self, offset: usize) -> VMResult<u8> {
        self.state.lock().memory.atomic_load_u8(offset)
    }

    #[inline(always)]
    pub fn atomic_load_u16(&self, offset: usize) -> VMResult<u16> {
        self.state.lock().memory.atomic_load_u16(offset)
    }

    #[inline(always)]
    pub fn atomic_load_u32(&self, offset: usize) -> VMResult<u32> {
        self.state.lock().memory.atomic_load_u32(offset)
    }

    #[inline(always)]
    pub fn atomic_load_u64(&self, offset: usize) -> VMResult<u64> {
        self.state.lock().memory.atomic_load_u64(offset)
    }

    #[inline(always)]
    pub fn atomic_store_u8(&self, offset: usize, value: u8) -> VMResult<()> {
        self.state.lock().memory.atomic_store_u8(offset, value)
    }

    #[inline(always)]
    pub fn atomic_store_u16(&self, offset: usize, value: u16) -> VMResult<()> {
        self.state.lock().memory.atomic_store_u16(offset, value)
    }

    #[inline(always)]
    pub fn atomic_store_u32(&self, offset: usize, value: u32) -> VMResult<()> {
        self.state.lock().memory.atomic_store_u32(offset, value)
    }

    #[inline(always)]
    pub fn atomic_store_u64(&self, offset: usize, value: u64) -> VMResult<()> {
        self.state.lock().memory.atomic_store_u64(offset, value)
    }

    #[inline(always)]
    pub(crate) fn atomic_rmw_u8(&self, offset: usize, op: AtomicRmwOp, value: u8) -> VMResult<u8> {
        self.state.lock().memory.atomic_rmw_u8(offset, op, value)
    }

    #[inline(always)]
    pub(crate) fn atomic_rmw_u16(
        &self,
        offset: usize,
        op: AtomicRmwOp,
        value: u16,
    ) -> VMResult<u16> {
        self.state.lock().memory.atomic_rmw_u16(offset, op, value)
    }

    #[inline(always)]
    pub(crate) fn atomic_rmw_u32(
        &self,
        offset: usize,
        op: AtomicRmwOp,
        value: u32,
    ) -> VMResult<u32> {
        self.state.lock().memory.atomic_rmw_u32(offset, op, value)
    }

    #[inline(always)]
    pub(crate) fn atomic_rmw_u64(
        &self,
        offset: usize,
        op: AtomicRmwOp,
        value: u64,
    ) -> VMResult<u64> {
        self.state.lock().memory.atomic_rmw_u64(offset, op, value)
    }

    #[inline(always)]
    pub fn atomic_cmpxchg_u8(&self, offset: usize, expected: u8, value: u8) -> VMResult<u8> {
        self.state
            .lock()
            .memory
            .atomic_cmpxchg_u8(offset, expected, value)
    }

    #[inline(always)]
    pub fn atomic_cmpxchg_u16(&self, offset: usize, expected: u16, value: u16) -> VMResult<u16> {
        self.state
            .lock()
            .memory
            .atomic_cmpxchg_u16(offset, expected, value)
    }

    #[inline(always)]
    pub fn atomic_cmpxchg_u32(&self, offset: usize, expected: u32, value: u32) -> VMResult<u32> {
        self.state
            .lock()
            .memory
            .atomic_cmpxchg_u32(offset, expected, value)
    }

    #[inline(always)]
    pub fn atomic_cmpxchg_u64(&self, offset: usize, expected: u64, value: u64) -> VMResult<u64> {
        self.state
            .lock()
            .memory
            .atomic_cmpxchg_u64(offset, expected, value)
    }

    #[inline(always)]
    pub fn atomic_fence(&self) {
        let _state = self.state.lock();
    }

    #[cfg(feature = "threads")]
    pub(crate) fn register_wait32(
        &self,
        offset: usize,
        expected: u32,
    ) -> VMResult<AtomicWaitResult> {
        let mut state = self.state.lock();
        vm_try!(ensure_atomic_alignment(offset, 4));
        let current = vm_try!(state.memory.atomic_load_u32(offset));
        if current != expected {
            return VMResult::Success(AtomicWaitResult::NotEqual);
        }
        let waiter = Arc::new(SharedWaiter::new(state.next_waiter_id));
        state.next_waiter_id += 1;
        state
            .wait_queues
            .entry(offset)
            .or_default()
            .push_back(waiter.clone());
        VMResult::Success(AtomicWaitResult::Pending(SharedWaitRegistration {
            address: offset,
            waiter,
        }))
    }

    #[cfg(feature = "threads")]
    pub(crate) fn register_wait64(
        &self,
        offset: usize,
        expected: u64,
    ) -> VMResult<AtomicWaitResult> {
        let mut state = self.state.lock();
        vm_try!(ensure_atomic_alignment(offset, 8));
        let current = vm_try!(state.memory.atomic_load_u64(offset));
        if current != expected {
            return VMResult::Success(AtomicWaitResult::NotEqual);
        }
        let waiter = Arc::new(SharedWaiter::new(state.next_waiter_id));
        state.next_waiter_id += 1;
        state
            .wait_queues
            .entry(offset)
            .or_default()
            .push_back(waiter.clone());
        VMResult::Success(AtomicWaitResult::Pending(SharedWaitRegistration {
            address: offset,
            waiter,
        }))
    }

    #[cfg(feature = "threads")]
    pub fn notify_waiters(&self, offset: usize, count: u32) -> VMResult<u32> {
        let mut state = self.state.lock();
        vm_try!(state.memory.atomic_load_u32(offset));
        let mut wake = Vec::new();
        let mut remaining = count;
        if let Some(queue) = state.wait_queues.get_mut(&offset) {
            while remaining != 0 {
                let Some(waiter) = queue.pop_front() else {
                    break;
                };
                if waiter.try_mark_notified() {
                    wake.push(waiter);
                    remaining -= 1;
                }
            }
            queue.retain(|waiter| waiter.is_waiting());
            if queue.is_empty() {
                state.wait_queues.remove(&offset);
            }
        }
        let woken = wake.len() as u32;
        drop(state);
        for waiter in wake {
            waiter.notify.notify_one();
        }
        VMResult::Success(woken)
    }

    #[cfg(feature = "threads")]
    fn remove_waiter(&self, offset: usize, waiter_id: u64) {
        let mut state = self.state.lock();
        if let Some(queue) = state.wait_queues.get_mut(&offset) {
            if let Some(index) = queue.iter().position(|waiter| waiter.id() == waiter_id) {
                queue.remove(index);
            }
            if queue.is_empty() {
                state.wait_queues.remove(&offset);
            }
        }
    }

    #[inline(always)]
    pub fn grow(&self, page_size_delta: u32) -> VMResult<i32> {
        self.state.lock().memory.grow(page_size_delta)
    }

    #[inline(always)]
    pub fn copy(&self, dst: u32, src: u32, len: u32) -> VMResult<()> {
        self.state.lock().memory.copy(dst, src, len)
    }

    #[inline(always)]
    pub fn fill(&self, ptr: u32, len: u32, data: u32) -> VMResult<()> {
        self.state.lock().memory.fill(ptr, len, data)
    }

    pub fn with_memory<T>(&self, f: impl FnOnce(&mut Memory) -> T) -> T {
        let mut state = self.state.lock();
        f(&mut state.memory)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_write_copy_fill_and_grow_match_linear_model_bytes() {
        let mut memory = Memory::new(1, 3).unwrap();

        assert!(matches!(
            memory.write_bytes(0, &[0x10, 0x20, 0x30, 0x40]),
            VMResult::Success(())
        ));
        assert!(matches!(memory.copy(8, 0, 4), VMResult::Success(())));
        assert!(matches!(memory.fill(12, 4, 0xaa), VMResult::Success(())));

        assert_eq!(
            &memory.slice()[0..16],
            &[
                0x10, 0x20, 0x30, 0x40, 0x00, 0x00, 0x00, 0x00, 0x10, 0x20, 0x30, 0x40, 0xaa, 0xaa,
                0xaa, 0xaa,
            ]
        );

        assert_eq!(memory.grow(1).unwrap(), 1);
        assert_eq!(memory.page_size(), 2);
        assert_eq!(
            &memory.slice()[0..16],
            &[
                0x10, 0x20, 0x30, 0x40, 0x00, 0x00, 0x00, 0x00, 0x10, 0x20, 0x30, 0x40, 0xaa, 0xaa,
                0xaa, 0xaa,
            ]
        );
        assert!(memory.slice()[PAGE_SIZE..PAGE_SIZE + 16]
            .iter()
            .all(|byte| *byte == 0));
    }

    #[test]
    fn memory_constructors_reject_page_count_larger_than_max() {
        assert!(matches!(
            Memory::new(2, 1),
            Err(MemoryInitError::InvalidPageBounds {
                page_count: 2,
                max_page_size: 1,
            })
        ));
        assert!(matches!(
            Memory::new_shared(2, 1),
            Err(MemoryInitError::InvalidPageBounds {
                page_count: 2,
                max_page_size: 1,
            })
        ));
        assert!(matches!(
            LocalMemoryObject::new(2, 1),
            Err(MemoryInitError::InvalidPageBounds {
                page_count: 2,
                max_page_size: 1,
            })
        ));
        assert!(matches!(
            SharedMemoryObject::new(2, 1),
            Err(MemoryInitError::InvalidPageBounds {
                page_count: 2,
                max_page_size: 1,
            })
        ));
    }

    #[test]
    fn reservation_bytes_are_checked_on_every_target() {
        let max_bytes = reservation_bytes_u64(PAGE_SIZE_MAX as u32);
        assert_eq!(max_bytes, 4_u64 * 1024 * 1024 * 1024);
        assert!(u32::try_from(max_bytes).is_err());
        #[cfg(target_pointer_width = "64")]
        assert_eq!(
            reservation_bytes(PAGE_SIZE_MAX as u32),
            Ok(max_bytes as usize)
        );
        #[cfg(target_pointer_width = "32")]
        assert_eq!(
            reservation_bytes(PAGE_SIZE_MAX as u32),
            Err(MemoryInitError::ReservationTooLarge {
                pages: PAGE_SIZE_MAX as u32,
            })
        );
        assert_eq!(reservation_bytes(0).unwrap(), PAGE_SIZE);
    }

    #[test]
    fn zero_sized_memory_keeps_a_valid_uncommitted_reservation() {
        let mut memory = Memory::new(0, 0).unwrap();
        assert_eq!(memory.page_size(), 0);
        assert_eq!(memory.data_size(), 0);
        assert_eq!(memory.reserved_bytes(), PAGE_SIZE);
        assert_eq!(memory.committed_bytes(), 0);
        assert_eq!(memory.grow(1).unwrap(), -1);
        assert_eq!(memory.committed_bytes(), 0);
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn local_memory_reserves_lazily_and_commits_only_grown_pages() {
        let max_pages = PAGE_SIZE_MAX as u32;
        let mut memory = Memory::new(1, max_pages).unwrap();
        assert_eq!(memory.reserved_bytes(), 4 * 1024 * 1024 * 1024usize);
        assert_eq!(memory.committed_bytes(), PAGE_SIZE);

        assert_eq!(memory.grow(1).unwrap(), 1);
        assert_eq!(memory.reserved_bytes(), 4 * 1024 * 1024 * 1024usize);
        assert_eq!(memory.committed_bytes(), 2 * PAGE_SIZE);
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn shared_memory_reserves_lazily_and_commits_only_grown_pages() {
        let max_pages = PAGE_SIZE_MAX as u32;
        let shared = SharedMemoryObject::new(1, max_pages).unwrap();
        shared.with_memory(|memory| {
            assert_eq!(memory.reserved_bytes(), 4 * 1024 * 1024 * 1024usize);
            assert_eq!(memory.committed_bytes(), PAGE_SIZE);
        });

        assert_eq!(shared.grow(1).unwrap(), 1);
        shared.with_memory(|memory| {
            assert_eq!(memory.reserved_bytes(), 4 * 1024 * 1024 * 1024usize);
            assert_eq!(memory.committed_bytes(), 2 * PAGE_SIZE);
        });
    }

    #[test]
    fn mmap_failure_is_returned_from_local_memory_construction() {
        let _failure = fail_next_memory_mapping(TestMemoryMappingFailure::Mmap);
        let error = LocalMemoryObject::new(1, 1).unwrap_err();
        assert!(matches!(
            error,
            MemoryInitError::MappingFailed {
                operation: MemoryMappingOperation::Mmap,
                bytes,
                errno: Some(_),
            } if bytes == PAGE_SIZE
        ));
    }

    #[test]
    fn initial_mprotect_failure_is_returned_from_local_memory_construction() {
        let _failure = fail_next_memory_mapping(TestMemoryMappingFailure::Mprotect);
        let error = LocalMemoryObject::new(1, 1).unwrap_err();
        assert!(matches!(
            error,
            MemoryInitError::MappingFailed {
                operation: MemoryMappingOperation::Mprotect,
                bytes,
                errno: Some(_),
            } if bytes == PAGE_SIZE
        ));
    }

    #[test]
    fn failed_grow_keeps_pages_and_commit_length_unchanged() {
        let mut memory = Memory::new(1, 2).unwrap();
        let _failure = fail_next_memory_mapping(TestMemoryMappingFailure::Mprotect);

        assert_eq!(memory.grow(1).unwrap(), -1);
        assert_eq!(memory.page_size(), 1);
        assert_eq!(memory.committed_bytes(), PAGE_SIZE);
    }

    #[cfg(feature = "threads")]
    #[tokio::test]
    async fn shared_wait_queue_internal_state_tracks_notify_and_timeout_cleanup() {
        let shared = SharedMemoryObject::new(1, 1).unwrap();
        assert!(matches!(
            shared.atomic_store_u32(0, 7),
            VMResult::Success(())
        ));

        let first = match shared.register_wait32(0, 7).unwrap() {
            AtomicWaitResult::Pending(wait) => wait,
            AtomicWaitResult::NotEqual => panic!("expected waiting registration"),
        };
        let second = match shared.register_wait32(0, 7).unwrap() {
            AtomicWaitResult::Pending(wait) => wait,
            AtomicWaitResult::NotEqual => panic!("expected waiting registration"),
        };

        {
            let state = shared.state.lock();
            let queue = state.wait_queues.get(&0).expect("queue must exist");
            assert_eq!(queue.len(), 2);
            assert!(queue.iter().all(|waiter| waiter.is_waiting()));
            assert_eq!(state.next_waiter_id, 3);
        }

        assert_eq!(shared.notify_waiters(0, 1).unwrap(), 1);
        assert_eq!(first.wait_result(shared.clone(), -1).await, 0);

        {
            let state = shared.state.lock();
            let queue = state.wait_queues.get(&0).expect("one waiter should remain");
            assert_eq!(queue.len(), 1);
            assert_eq!(queue.front().unwrap().id(), second.waiter.id());
            assert!(queue.front().unwrap().is_waiting());
        }

        assert_eq!(second.wait_result(shared.clone(), 0).await, 2);

        {
            let state = shared.state.lock();
            assert!(!state.wait_queues.contains_key(&0));
        }
        assert_eq!(shared.notify_waiters(0, 1).unwrap(), 0);
    }

    #[cfg(feature = "threads")]
    #[tokio::test]
    async fn shared_wait_queue_rejects_mismatch_and_notifies_fifo_up_to_count() {
        let shared = SharedMemoryObject::new(1, 1).unwrap();
        assert!(matches!(
            shared.atomic_store_u32(0, 11),
            VMResult::Success(())
        ));

        assert!(matches!(
            shared.register_wait32(0, 12).unwrap(),
            AtomicWaitResult::NotEqual
        ));

        let first = match shared.register_wait32(0, 11).unwrap() {
            AtomicWaitResult::Pending(wait) => wait,
            AtomicWaitResult::NotEqual => panic!("expected waiting registration"),
        };
        let second = match shared.register_wait32(0, 11).unwrap() {
            AtomicWaitResult::Pending(wait) => wait,
            AtomicWaitResult::NotEqual => panic!("expected waiting registration"),
        };
        let third = match shared.register_wait32(0, 11).unwrap() {
            AtomicWaitResult::Pending(wait) => wait,
            AtomicWaitResult::NotEqual => panic!("expected waiting registration"),
        };

        assert_eq!(shared.notify_waiters(0, 2).unwrap(), 2);
        assert_eq!(first.wait_result(shared.clone(), 0).await, 0);
        assert_eq!(second.wait_result(shared.clone(), 0).await, 0);

        {
            let state = shared.state.lock();
            let queue = state.wait_queues.get(&0).expect("one waiter should remain");
            assert_eq!(queue.len(), 1);
            assert_eq!(queue.front().unwrap().id(), third.waiter.id());
        }

        assert_eq!(shared.notify_waiters(0, 10).unwrap(), 1);
        assert_eq!(third.wait_result(shared.clone(), 0).await, 0);
        assert_eq!(shared.notify_waiters(0, 1).unwrap(), 0);
    }

    #[cfg(feature = "threads")]
    #[tokio::test]
    async fn shared_wait64_queue_tracks_notify_and_timeout_cleanup() {
        let shared = SharedMemoryObject::new(1, 1).unwrap();
        let value = 0x0102_0304_0506_0708u64;
        assert!(matches!(
            shared.atomic_store_u64(8, value),
            VMResult::Success(())
        ));

        let first = match shared.register_wait64(8, value).unwrap() {
            AtomicWaitResult::Pending(wait) => wait,
            AtomicWaitResult::NotEqual => panic!("expected waiting registration"),
        };
        let second = match shared.register_wait64(8, value).unwrap() {
            AtomicWaitResult::Pending(wait) => wait,
            AtomicWaitResult::NotEqual => panic!("expected waiting registration"),
        };

        assert_eq!(shared.notify_waiters(8, 1).unwrap(), 1);
        assert_eq!(first.wait_result(shared.clone(), -1).await, 0);

        {
            let state = shared.state.lock();
            let queue = state.wait_queues.get(&8).expect("one waiter should remain");
            assert_eq!(queue.len(), 1);
            assert_eq!(queue.front().unwrap().id(), second.waiter.id());
            assert!(queue.front().unwrap().is_waiting());
        }

        assert_eq!(second.wait_result(shared.clone(), 0).await, 2);

        {
            let state = shared.state.lock();
            assert!(!state.wait_queues.contains_key(&8));
        }
        assert_eq!(shared.notify_waiters(8, 1).unwrap(), 0);
    }
    #[cfg(feature = "threads")]
    #[test]
    fn shared_wait_timeout_completes_on_non_tokio_executor() {
        // #151: a non-negative timeout used to be implemented with
        // tokio::time::sleep, which panics ("no reactor running") when an
        // embedder drives execution outside a Tokio runtime. Drive every
        // wait below with futures::executor::block_on — no Tokio reactor —
        // and require each wait to complete normally instead of panicking.
        let shared = SharedMemoryObject::new(1, 1).unwrap();
        assert!(matches!(
            shared.atomic_store_u32(0, 7),
            VMResult::Success(())
        ));
        let register =
            |shared: &Arc<SharedMemoryObject>| match shared.register_wait32(0, 7).unwrap() {
                AtomicWaitResult::Pending(wait) => wait,
                AtomicWaitResult::NotEqual => panic!("expected waiting registration"),
            };

        // A finite timeout expires on its own and reports the timed-out
        // result (2), removing the waiter from the queue.
        let timed_out = register(&shared);
        assert_eq!(
            futures::executor::block_on(timed_out.wait_result(shared.clone(), 1_000_000)),
            2
        );
        {
            let state = shared.state.lock();
            assert!(!state.wait_queues.contains_key(&0));
        }

        // A zero timeout also resolves as timed out when nobody notifies.
        let zero = register(&shared);
        assert_eq!(
            futures::executor::block_on(zero.wait_result(shared.clone(), 0)),
            2
        );

        // A waiter notified before its (long) timeout elapses still reports
        // the notified result (0).
        let notified = register(&shared);
        assert_eq!(shared.notify_waiters(0, 1).unwrap(), 1);
        assert_eq!(
            futures::executor::block_on(notified.wait_result(shared.clone(), 60_000_000_000)),
            0
        );

        // timeout = -1 keeps its existing behavior: it waits indefinitely
        // and resolves as notified (0) once a notify lands.
        let indefinite = register(&shared);
        assert_eq!(shared.notify_waiters(0, 1).unwrap(), 1);
        assert_eq!(
            futures::executor::block_on(indefinite.wait_result(shared.clone(), -1)),
            0
        );
    }

    #[cfg(feature = "threads")]
    #[test]
    fn interruptible_wait_is_woken_by_a_metering_interrupt() {
        // #177: an indefinite wait used to observe only its own Notify, so
        // MeteringHandle::interrupt() could never evict the parked guest.
        let shared = SharedMemoryObject::new(1, 1).unwrap();
        assert!(matches!(
            shared.atomic_store_u32(0, 7),
            VMResult::Success(())
        ));
        let register =
            |shared: &Arc<SharedMemoryObject>| match shared.register_wait32(0, 7).unwrap() {
                AtomicWaitResult::Pending(wait) => wait,
                AtomicWaitResult::NotEqual => panic!("expected waiting registration"),
            };

        // The interrupt lands while the guest is parked: the wait resolves
        // as Cancelled (not as a wait result code) and the waiter is
        // removed from the queue, so a later notify finds nobody.
        let meter = MeteringHandle::new(None);
        let parked = register(&shared);
        let interrupter = meter.clone();
        let interrupt_thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            interrupter.interrupt();
        });
        let outcome = futures::executor::block_on(parked.wait_result_interruptible(
            shared.clone(),
            -1,
            Some(meter),
        ));
        interrupt_thread.join().unwrap();
        assert!(matches!(outcome, VMResult::Cancelled));
        {
            let state = shared.state.lock();
            assert!(!state.wait_queues.contains_key(&0));
        }
        assert_eq!(shared.notify_waiters(0, 1).unwrap(), 0);

        // An interrupt requested before the wait even starts is observed
        // immediately rather than hanging on the broadcast.
        let meter = MeteringHandle::new(None);
        meter.interrupt();
        let parked = register(&shared);
        let outcome = futures::executor::block_on(parked.wait_result_interruptible(
            shared.clone(),
            -1,
            Some(meter),
        ));
        assert!(matches!(outcome, VMResult::Cancelled));
    }

    #[cfg(feature = "threads")]
    #[test]
    fn interruptible_wait_keeps_notify_and_timeout_result_codes() {
        // #177 acceptance: attaching an interrupt source must not change
        // the existing result codes — notify 0, timeout 2, and a notify
        // that claims the waiter before the interrupt keeps its 0.
        let shared = SharedMemoryObject::new(1, 1).unwrap();
        assert!(matches!(
            shared.atomic_store_u32(0, 7),
            VMResult::Success(())
        ));
        let register =
            |shared: &Arc<SharedMemoryObject>| match shared.register_wait32(0, 7).unwrap() {
                AtomicWaitResult::Pending(wait) => wait,
                AtomicWaitResult::NotEqual => panic!("expected waiting registration"),
            };

        // The timeout still fires when the interrupt never does.
        let meter = MeteringHandle::new(None);
        let timed_out = register(&shared);
        assert!(matches!(
            futures::executor::block_on(timed_out.wait_result_interruptible(
                shared.clone(),
                1_000_000,
                Some(meter),
            )),
            VMResult::Success(2)
        ));

        // A notify that lands first claims the waiter; an interrupt
        // requested afterwards cannot rewrite the notified result.
        let meter = MeteringHandle::new(None);
        let notified = register(&shared);
        assert_eq!(shared.notify_waiters(0, 1).unwrap(), 1);
        meter.interrupt();
        assert!(matches!(
            futures::executor::block_on(notified.wait_result_interruptible(
                shared.clone(),
                -1,
                Some(meter),
            )),
            VMResult::Success(0)
        ));
    }

    #[cfg(feature = "threads")]
    #[test]
    fn resolved_long_waits_release_their_timer_threads() {
        // #270: the detached timer thread armed for a finite timeout used
        // to sleep until the original deadline even when a notify or an
        // interrupt resolved the wait immediately, so promptly-resolved
        // long-timeout waits accumulated OS threads without bound (until
        // thread::spawn panicked in the host). Timer threads must be
        // released as soon as their wait resolves, however it resolves.
        let shared = SharedMemoryObject::new(1, 1).unwrap();
        assert!(matches!(
            shared.atomic_store_u32(0, 7),
            VMResult::Success(())
        ));
        let register =
            |shared: &Arc<SharedMemoryObject>| match shared.register_wait32(0, 7).unwrap() {
                AtomicWaitResult::Pending(wait) => wait,
                AtomicWaitResult::NotEqual => panic!("expected waiting registration"),
            };
        // One hour: with the defect, every timer thread armed below is
        // still asleep at this deadline long after the test gives up.
        const LONG_TIMEOUT_NS: i64 = 3_600_000_000_000;

        let baseline = live_wait_timer_threads();

        // Waits notified before wait_result is even polled.
        for _ in 0..16 {
            let wait = register(&shared);
            assert_eq!(shared.notify_waiters(0, 1).unwrap(), 1);
            assert_eq!(
                futures::executor::block_on(wait.wait_result(shared.clone(), LONG_TIMEOUT_NS)),
                0
            );
        }

        // Waits notified while parked, with the timer already armed.
        for _ in 0..16 {
            let wait = register(&shared);
            let notify_shared = shared.clone();
            let notifier = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(20));
                assert_eq!(notify_shared.notify_waiters(0, 1).unwrap(), 1);
            });
            assert_eq!(
                futures::executor::block_on(wait.wait_result(shared.clone(), LONG_TIMEOUT_NS)),
                0
            );
            notifier.join().unwrap();
        }

        // Waits cancelled by a metering interrupt (#177) while parked.
        for _ in 0..4 {
            let meter = MeteringHandle::new(None);
            let wait = register(&shared);
            let interrupter_meter = meter.clone();
            let interrupter = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(20));
                interrupter_meter.interrupt();
            });
            let outcome = futures::executor::block_on(wait.wait_result_interruptible(
                shared.clone(),
                LONG_TIMEOUT_NS,
                Some(meter),
            ));
            interrupter.join().unwrap();
            assert!(matches!(outcome, VMResult::Cancelled));
        }

        // Every timer thread armed above must be released promptly, not
        // at the one-hour deadline. Poll for up to ~10 seconds.
        for _ in 0..1_000 {
            if live_wait_timer_threads() <= baseline {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            live_wait_timer_threads() <= baseline,
            "wait timer threads accumulated: {} live (baseline {baseline})",
            live_wait_timer_threads()
        );
    }
}
