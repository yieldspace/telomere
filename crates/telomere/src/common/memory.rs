use std::{
    collections::{HashMap, VecDeque},
    fmt,
    ptr::NonNull,
    slice::SliceIndex,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
    time::Duration,
};

use parking_lot::Mutex;
use tokio::sync::Notify;
use vstd::prelude::*;

use super::{Stack, VMResult, PAGE_SIZE};

verus! {

pub open spec fn spec_offset_result(memarg_offset: u32, offset: u32) -> Option<int> {
    if memarg_offset as int + offset as int <= u32::MAX as int {
        Some(memarg_offset as int + offset as int)
    } else {
        None
    }
}

#[inline(always)]
pub exec fn checked_memory_offset(memarg_offset: u32, offset: u32) -> (result: Option<usize>)
    ensures
        match spec_offset_result(memarg_offset, offset) {
            Some(value) => result == Some(value as usize),
            None => result == Option::<usize>::None,
        },
{
    let sum = memarg_offset as u64 + offset as u64;
    if sum <= u32::MAX as u64 {
        Some(sum as usize)
    } else {
        None
    }
}

#[inline(always)]
pub exec fn atomic_alignment_valid(offset: usize, alignment: usize) -> (result: bool)
    requires
        alignment != 0,
    ensures
        result == (offset % alignment == 0),
{
    offset % alignment == 0
}

pub open spec fn spec_write_range(data: Seq<u8>, start: int, bytes: Seq<u8>) -> Seq<u8> {
    Seq::new(
        data.len(),
        |i: int| {
            if start <= i && i < start + bytes.len() {
                bytes[i - start]
            } else {
                data[i]
            }
        },
    )
}

pub open spec fn spec_fill_range(data: Seq<u8>, start: int, len: int, value: u8) -> Seq<u8> {
    Seq::new(
        data.len(),
        |i: int| {
            if start <= i && i < start + len {
                value
            } else {
                data[i]
            }
        },
    )
}

pub open spec fn spec_copy_within_range(data: Seq<u8>, dst: int, src: int, len: int) -> Seq<u8> {
    spec_write_range(data, dst, data.subrange(src, src + len))
}

pub open spec fn spec_read_range(data: Seq<u8>, start: int, len: int) -> Seq<u8>
    recommends
        0 <= start,
        0 <= len,
        start + len <= data.len(),
{
    data.subrange(start, start + len)
}

pub closed spec fn spec_le_u16(bytes: Seq<u8>) -> u16
    recommends
        bytes.len() == 2,
{
    (bytes[0] as u16) | ((bytes[1] as u16) << 8)
}

pub closed spec fn spec_le_u32(bytes: Seq<u8>) -> u32
    recommends
        bytes.len() == 4,
{
    (bytes[0] as u32)
        | ((bytes[1] as u32) << 8)
        | ((bytes[2] as u32) << 16)
        | ((bytes[3] as u32) << 24)
}

pub closed spec fn spec_le_u64(bytes: Seq<u8>) -> u64
    recommends
        bytes.len() == 8,
{
    (bytes[0] as u64)
        | ((bytes[1] as u64) << 8)
        | ((bytes[2] as u64) << 16)
        | ((bytes[3] as u64) << 24)
        | ((bytes[4] as u64) << 32)
        | ((bytes[5] as u64) << 40)
        | ((bytes[6] as u64) << 48)
        | ((bytes[7] as u64) << 56)
}

pub closed spec fn spec_le_u128(bytes: Seq<u8>) -> u128
    recommends
        bytes.len() == 16,
{
    (bytes[0] as u128)
        | ((bytes[1] as u128) << 8)
        | ((bytes[2] as u128) << 16)
        | ((bytes[3] as u128) << 24)
        | ((bytes[4] as u128) << 32)
        | ((bytes[5] as u128) << 40)
        | ((bytes[6] as u128) << 48)
        | ((bytes[7] as u128) << 56)
        | ((bytes[8] as u128) << 64)
        | ((bytes[9] as u128) << 72)
        | ((bytes[10] as u128) << 80)
        | ((bytes[11] as u128) << 88)
        | ((bytes[12] as u128) << 96)
        | ((bytes[13] as u128) << 104)
        | ((bytes[14] as u128) << 112)
        | ((bytes[15] as u128) << 120)
}

pub proof fn lemma_write_range_preserves_len(data: Seq<u8>, start: int, bytes: Seq<u8>)
    ensures
        spec_write_range(data, start, bytes).len() == data.len(),
{
}

pub proof fn lemma_fill_range_preserves_len(data: Seq<u8>, start: int, len: int, value: u8)
    ensures
        spec_fill_range(data, start, len, value).len() == data.len(),
{
}

pub proof fn lemma_copy_within_preserves_len(data: Seq<u8>, dst: int, src: int, len: int)
    ensures
        spec_copy_within_range(data, dst, src, len).len() == data.len(),
{
}

pub proof fn lemma_write_range_updates_written_bytes(data: Seq<u8>, start: int, bytes: Seq<u8>, i: int)
    requires
        0 <= start,
        0 <= i < bytes.len(),
        start + bytes.len() <= data.len(),
    ensures
        spec_write_range(data, start, bytes)[start + i] == bytes[i],
{
}

pub proof fn lemma_fill_range_updates_written_bytes(data: Seq<u8>, start: int, len: int, value: u8, i: int)
    requires
        0 <= start,
        0 <= i < len,
        start + len <= data.len(),
    ensures
        spec_fill_range(data, start, len, value)[start + i] == value,
{
}

pub proof fn lemma_write_range_preserves_outside_bytes(data: Seq<u8>, start: int, bytes: Seq<u8>, i: int)
    requires
        0 <= i < data.len(),
        i < start || start + bytes.len() <= i,
    ensures
        spec_write_range(data, start, bytes)[i] == data[i],
{
}

pub proof fn lemma_fill_range_preserves_outside_bytes(data: Seq<u8>, start: int, len: int, value: u8, i: int)
    requires
        0 <= i < data.len(),
        i < start || start + len <= i,
    ensures
        spec_fill_range(data, start, len, value)[i] == data[i],
{
}

#[verifier::external_body]
#[inline(always)]
pub exec fn trusted_copy_from_slice(dst: &mut [u8], src: &[u8])
    requires
        old(dst)@.len() == src@.len(),
    ensures
        dst@ == src@,
{
    dst.copy_from_slice(src);
}

#[verifier::external_body]
#[inline(always)]
pub exec fn trusted_fill_slice(dst: &mut [u8], value: u8)
    ensures
        dst@ == spec_fill_range(old(dst)@, 0, old(dst)@.len() as int, value),
{
    dst.fill(value);
}

#[verifier::external_body]
#[inline(always)]
pub exec fn trusted_copy_within(dst: &mut [u8], src_start: usize, src_end: usize, dst_start: usize)
    requires
        src_start <= src_end,
        src_end as int <= old(dst)@.len(),
        dst_start as int + (src_end as int - src_start as int) <= old(dst)@.len(),
    ensures
        dst@ == spec_copy_within_range(
            old(dst)@,
            dst_start as int,
            src_start as int,
            src_end as int - src_start as int,
        ),
{
    dst.copy_within(src_start..src_end, dst_start);
}

#[verifier::external_body]
#[inline(always)]
pub exec fn trusted_read_u16(src: &[u8]) -> (value: u16)
    requires
        src@.len() == 2,
    ensures
        value == spec_le_u16(src@),
{
    unsafe { u16::from_le(src.as_ptr().cast::<u16>().read_unaligned()) }
}

#[verifier::external_body]
#[inline(always)]
pub exec fn trusted_read_u32(src: &[u8]) -> (value: u32)
    requires
        src@.len() == 4,
    ensures
        value == spec_le_u32(src@),
{
    unsafe { u32::from_le(src.as_ptr().cast::<u32>().read_unaligned()) }
}

#[verifier::external_body]
#[inline(always)]
pub exec fn trusted_read_u64(src: &[u8]) -> (value: u64)
    requires
        src@.len() == 8,
    ensures
        value == spec_le_u64(src@),
{
    unsafe { u64::from_le(src.as_ptr().cast::<u64>().read_unaligned()) }
}

#[verifier::external_body]
#[inline(always)]
pub exec fn trusted_read_u128(src: &[u8]) -> (value: u128)
    requires
        src@.len() == 16,
    ensures
        value == spec_le_u128(src@),
{
    unsafe { u128::from_le(src.as_ptr().cast::<u128>().read_unaligned()) }
}

} // verus!

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

verus! {

pub closed spec fn spec_atomic_cmpxchg_u8(old: u8, expected: u8, value: u8) -> u8 {
    if old == expected { value } else { old }
}

pub closed spec fn spec_atomic_cmpxchg_u16(old: u16, expected: u16, value: u16) -> u16 {
    if old == expected { value } else { old }
}

pub closed spec fn spec_atomic_cmpxchg_u32(old: u32, expected: u32, value: u32) -> u32 {
    if old == expected { value } else { old }
}

pub closed spec fn spec_atomic_cmpxchg_u64(old: u64, expected: u64, value: u64) -> u64 {
    if old == expected { value } else { old }
}

pub open spec fn spec_wait_queue_push(queue: Seq<u64>, waiter_id: u64) -> Seq<u64> {
    queue.push(waiter_id)
}

pub closed spec fn spec_wait_queue_wake_count(queue_len: nat, count: u32) -> nat {
    if queue_len < count as nat {
        queue_len
    } else {
        count as nat
    }
}

pub open spec fn spec_wait_queue_remaining(queue: Seq<u64>, count: u32) -> Seq<u64>
    recommends
        spec_wait_queue_wake_count(queue.len() as nat, count) <= queue.len(),
{
    queue.subrange(
        spec_wait_queue_wake_count(queue.len() as nat, count) as int,
        queue.len() as int,
    )
}

pub proof fn lemma_wait_queue_push_appends(queue: Seq<u64>, waiter_id: u64)
    ensures
        spec_wait_queue_push(queue, waiter_id).len() == queue.len() + 1,
        spec_wait_queue_push(queue, waiter_id)[queue.len() as int] == waiter_id,
{
}

pub proof fn lemma_wait_queue_notify_count_bounded(queue_len: nat, count: u32)
    ensures
        spec_wait_queue_wake_count(queue_len, count) <= queue_len,
        spec_wait_queue_wake_count(queue_len, count) <= count as nat,
{
}

} // verus!

#[derive(Debug)]
pub struct SharedWaitRegistration {
    address: usize,
    waiter: Arc<SharedWaiter>,
}

impl SharedWaitRegistration {
    pub fn address(&self) -> usize {
        self.address
    }

    pub async fn wait_result(self, shared: Arc<SharedMemoryObject>, timeout_ns: i64) -> i32 {
        if timeout_ns < 0 {
            self.waiter.wait().await;
            return 0;
        }

        let sleep = tokio::time::sleep(Duration::from_nanos(timeout_ns as u64));
        tokio::pin!(sleep);
        tokio::select! {
            _ = self.waiter.wait() => 0,
            _ = &mut sleep => {
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

#[derive(Debug)]
pub enum AtomicWaitResult {
    NotEqual,
    Pending(SharedWaitRegistration),
}

#[derive(Debug)]
struct SharedWaiter {
    id: u64,
    state: AtomicU8,
    notify: Notify,
}

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

pub struct Memory {
    region: MmapRegion,
    current_pages: u32,
    max_pages: u32,
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
    pub fn new(page_count: u32, max_page_size: u32) -> Self {
        Self::new_with_mapping(page_count, max_page_size, false)
    }

    pub fn new_shared(page_count: u32, max_page_size: u32) -> Self {
        Self::new_with_mapping(page_count, max_page_size, true)
    }

    fn new_with_mapping(page_count: u32, max_page_size: u32, shared: bool) -> Self {
        let reserved = (max_page_size as usize * PAGE_SIZE).max(PAGE_SIZE);
        let region = MmapRegion::new(reserved, shared);
        Self {
            region,
            current_pages: page_count,
            max_pages: max_page_size,
        }
    }

    pub fn page_size(&self) -> u32 {
        self.current_pages
    }

    pub fn data_size(&self) -> usize {
        self.current_pages as usize * PAGE_SIZE
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
    pub fn push_to_stack<const N: usize>(&self, stack: &mut Stack, offset: usize) -> VMResult<()> {
        let last = vm_try!(VMResult::from_option(offset.checked_add(N), || {
            VMResult::MemoryIndexOutOfRange
        }));
        let bytes = vm_try!(VMResult::from_option(
            self.slice().get(offset..last),
            || { VMResult::MemoryIndexOutOfRange }
        ));
        stack.push_slice(bytes)
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
        VMResult::Success(*vm_try!(VMResult::from_option(
            self.slice().get(offset),
            || VMResult::MemoryIndexOutOfRange
        )))
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
        let bytes = vm_try!(VMResult::from_option(
            self.slice().get(offset..last),
            || { VMResult::MemoryIndexOutOfRange }
        ));
        VMResult::Success(trusted_read_u16(bytes))
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
        let bytes = vm_try!(VMResult::from_option(
            self.slice().get(offset..last),
            || { VMResult::MemoryIndexOutOfRange }
        ));
        VMResult::Success(trusted_read_u32(bytes))
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
        let bytes = vm_try!(VMResult::from_option(
            self.slice().get(offset..last),
            || { VMResult::MemoryIndexOutOfRange }
        ));
        VMResult::Success(trusted_read_u64(bytes))
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
        let new_page_size = current_page_size + page_size_delta;
        if self.max_pages >= new_page_size {
            self.current_pages = new_page_size;
            VMResult::Success(current_page_size as i32)
        } else {
            VMResult::Success(-1)
        }
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
    pub fn new(page_count: u32, max_page_size: u32) -> Self {
        Self {
            memory: Memory::new(page_count, max_page_size),
        }
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
    wait_queues: HashMap<usize, VecDeque<Arc<SharedWaiter>>>,
    next_waiter_id: u64,
}

#[derive(Debug)]
pub struct SharedMemoryObject {
    state: Mutex<SharedMemoryState>,
}

impl SharedMemoryObject {
    pub fn new(page_count: u32, max_page_size: u32) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(SharedMemoryState {
                memory: Memory::new_shared(page_count, max_page_size),
                wait_queues: HashMap::new(),
                next_waiter_id: 1,
            }),
        })
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
