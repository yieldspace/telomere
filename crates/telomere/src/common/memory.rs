use std::{fmt, ptr::NonNull, slice::SliceIndex, sync::Arc};

#[cfg(feature = "threads")]
use std::{
    collections::{HashMap, VecDeque},
    sync::atomic::{AtomicU8, Ordering},
    time::Duration,
};

use parking_lot::Mutex;
#[cfg(feature = "threads")]
use tokio::sync::Notify;

use super::{Stack, VMResult, PAGE_SIZE};

#[inline(always)]
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
pub enum AtomicRmwOp {
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
    pub fn address(&self) -> usize {
        self.address
    }

    pub async fn wait_result(self, shared: Arc<SharedMemoryObject>, timeout_ns: i64) -> i32 {
        if timeout_ns < 0 {
            self.waiter.wait().await;
            return 0;
        }

        let wait = std::pin::pin!(self.waiter.wait());
        let timeout = std::pin::pin!(tokio::time::sleep(Duration::from_nanos(timeout_ns as u64)));
        match futures::future::select(wait, timeout).await {
            futures::future::Either::Left(((), _)) => 0,
            futures::future::Either::Right(((), _)) => {
                if self.waiter.try_mark_timed_out() {
                    shared.remove_waiter(self.address, self.waiter.id());
                    2
                } else {
                    0
                }
            }
        }
    }
}

#[cfg(feature = "threads")]
#[derive(Debug)]
pub enum AtomicWaitResult {
    NotEqual,
    Pending(SharedWaitRegistration),
}

#[cfg(feature = "threads")]
#[derive(Debug)]
struct SharedWaiter {
    id: u64,
    state: AtomicU8,
    notify: Notify,
}

#[cfg(feature = "threads")]
impl SharedWaiter {
    const WAITING: u8 = 0;
    const NOTIFIED: u8 = 1;
    const TIMED_OUT: u8 = 2;

    fn new(id: u64) -> Self {
        Self {
            id,
            state: AtomicU8::new(Self::WAITING),
            notify: Notify::new(),
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
        self.state
            .compare_exchange(
                Self::WAITING,
                Self::NOTIFIED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
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

    fn is_waiting(&self) -> bool {
        self.state.load(Ordering::Acquire) == Self::WAITING
    }
}

#[derive(Debug)]
struct MmapRegion {
    ptr: NonNull<u8>,
    len: usize,
}

impl MmapRegion {
    fn new(len: usize, shared: bool) -> Self {
        assert!(len != 0, "mmap region length must be non-zero");
        let mut flags = libc::MAP_ANON;
        flags |= if shared {
            libc::MAP_SHARED
        } else {
            libc::MAP_PRIVATE
        };
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                flags,
                -1,
                0,
            )
        };
        assert_ne!(ptr, libc::MAP_FAILED, "mmap failed for {len} bytes");
        Self {
            ptr: NonNull::new(ptr.cast::<u8>()).expect("mmap returned null"),
            len,
        }
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
        let ret = unsafe { libc::munmap(self.ptr.as_ptr().cast::<libc::c_void>(), self.len) };
        debug_assert_eq!(ret, 0, "munmap failed");
    }
}

// SAFETY: the mapping owns its virtual memory reservation and raw pointer metadata only.
// Cross-thread access is synchronized by the caller (`Store` single-flight or shared-memory mutex).
unsafe impl Send for MmapRegion {}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryInitError {
    InvalidPageBounds { page_count: u32, max_page_size: u32 },
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
        }
    }
}

impl std::error::Error for MemoryInitError {}

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

impl Memory {
    #[inline(always)]
    fn base_ptr(&self) -> *const u8 {
        self.region.ptr.as_ptr()
    }

    pub fn new(page_count: u32, max_page_size: u32) -> Result<Self, MemoryInitError> {
        Self::new_with_mapping(page_count, max_page_size, false)
    }

    pub fn new_shared(page_count: u32, max_page_size: u32) -> Result<Self, MemoryInitError> {
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
        let reserved = (max_page_size as usize * PAGE_SIZE).max(PAGE_SIZE);
        let region = MmapRegion::new(reserved, shared);
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
        self.current_pages as usize * PAGE_SIZE
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
    pub fn atomic_rmw_u8(&mut self, offset: usize, op: AtomicRmwOp, value: u8) -> VMResult<u8> {
        let old = vm_try!(self.atomic_load_u8(offset));
        vm_try!(self.atomic_store_u8(offset, op.apply_u8(old, value)));
        VMResult::Success(old)
    }

    #[inline(always)]
    pub fn atomic_rmw_u16(&mut self, offset: usize, op: AtomicRmwOp, value: u16) -> VMResult<u16> {
        let old = vm_try!(self.atomic_load_u16(offset));
        vm_try!(self.atomic_store_u16(offset, op.apply_u16(old, value)));
        VMResult::Success(old)
    }

    #[inline(always)]
    pub fn atomic_rmw_u32(&mut self, offset: usize, op: AtomicRmwOp, value: u32) -> VMResult<u32> {
        let old = vm_try!(self.atomic_load_u32(offset));
        vm_try!(self.atomic_store_u32(offset, op.apply_u32(old, value)));
        VMResult::Success(old)
    }

    #[inline(always)]
    pub fn atomic_rmw_u64(&mut self, offset: usize, op: AtomicRmwOp, value: u64) -> VMResult<u64> {
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
        self.current_pages = new_page_size;
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
pub struct LocalMemoryObject {
    memory: Memory,
}

impl LocalMemoryObject {
    pub fn new(page_count: u32, max_page_size: u32) -> Result<Self, MemoryInitError> {
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
    pub fn atomic_rmw_u8(&mut self, offset: usize, op: AtomicRmwOp, value: u8) -> VMResult<u8> {
        self.memory.atomic_rmw_u8(offset, op, value)
    }

    #[inline(always)]
    pub fn atomic_rmw_u16(&mut self, offset: usize, op: AtomicRmwOp, value: u16) -> VMResult<u16> {
        self.memory.atomic_rmw_u16(offset, op, value)
    }

    #[inline(always)]
    pub fn atomic_rmw_u32(&mut self, offset: usize, op: AtomicRmwOp, value: u32) -> VMResult<u32> {
        self.memory.atomic_rmw_u32(offset, op, value)
    }

    #[inline(always)]
    pub fn atomic_rmw_u64(&mut self, offset: usize, op: AtomicRmwOp, value: u64) -> VMResult<u64> {
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
    pub fn new(page_count: u32, max_page_size: u32) -> Result<Arc<Self>, MemoryInitError> {
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
    pub fn atomic_rmw_u8(&self, offset: usize, op: AtomicRmwOp, value: u8) -> VMResult<u8> {
        self.state.lock().memory.atomic_rmw_u8(offset, op, value)
    }

    #[inline(always)]
    pub fn atomic_rmw_u16(&self, offset: usize, op: AtomicRmwOp, value: u16) -> VMResult<u16> {
        self.state.lock().memory.atomic_rmw_u16(offset, op, value)
    }

    #[inline(always)]
    pub fn atomic_rmw_u32(&self, offset: usize, op: AtomicRmwOp, value: u32) -> VMResult<u32> {
        self.state.lock().memory.atomic_rmw_u32(offset, op, value)
    }

    #[inline(always)]
    pub fn atomic_rmw_u64(&self, offset: usize, op: AtomicRmwOp, value: u64) -> VMResult<u64> {
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
    pub fn register_wait32(&self, offset: usize, expected: u32) -> VMResult<AtomicWaitResult> {
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
    pub fn register_wait64(&self, offset: usize, expected: u64) -> VMResult<AtomicWaitResult> {
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
}
