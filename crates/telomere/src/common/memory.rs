use std::{
    collections::{HashMap, HashSet, VecDeque},
    fmt,
    ptr::NonNull,
    slice::SliceIndex,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
    task::{Context, Poll},
};

use futures::task::AtomicWaker;
use parking_lot::Mutex;
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

    pub fn poll_wait(&self, cx: &mut Context<'_>) -> Poll<()> {
        self.waiter.poll_wait(cx)
    }

    pub(crate) fn finish_notified_protocol(
        self,
        shared: &SharedMemoryObject,
    ) -> SharedAtomicProtocolResult<i32> {
        shared.consume_wake(self.address, self.waiter.id())
    }

    pub fn finish_notified(self, shared: &SharedMemoryObject) -> i32 {
        self.finish_notified_protocol(shared).result
    }

    pub(crate) fn finish_timeout_protocol(
        self,
        shared: &SharedMemoryObject,
    ) -> SharedAtomicProtocolResult<i32> {
        if self.waiter.try_mark_timed_out() {
            shared.consume_timed_out(self.address, self.waiter.id())
        } else {
            shared.consume_wake(self.address, self.waiter.id())
        }
    }

    pub fn finish_timeout(self, shared: &SharedMemoryObject) -> i32 {
        self.finish_timeout_protocol(shared).result
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
    waker: AtomicWaker,
}

impl SharedWaiter {
    const WAITING: u8 = 0;
    const NOTIFIED: u8 = 1;
    const TIMED_OUT: u8 = 2;

    fn new(id: u64) -> Self {
        Self {
            id,
            state: AtomicU8::new(Self::WAITING),
            waker: AtomicWaker::new(),
        }
    }

    fn id(&self) -> u64 {
        self.id
    }

    fn poll_wait(&self, cx: &mut Context<'_>) -> Poll<()> {
        if self.state.load(Ordering::Acquire) == Self::NOTIFIED {
            return Poll::Ready(());
        }
        self.waker.register(cx.waker());
        if self.state.load(Ordering::Acquire) == Self::NOTIFIED {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
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

    fn wake(&self) {
        self.waker.wake();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SharedWaitStateProjection {
    Waiting,
    Notified,
    TimedOut,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LinearMemoryProjection {
    pub(crate) bytes: Vec<u8>,
    pub(crate) current_pages: u32,
    pub(crate) max_pages: u32,
    pub(crate) shared: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct LinearMemoryProjectionParts {
    pub(crate) bytes: Vec<u8>,
    pub(crate) current_pages: u32,
    pub(crate) max_pages: u32,
    pub(crate) shared: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SharedWaitQueueProjection {
    pub(crate) address: usize,
    pub(crate) waiter_ids: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SharedWaiterProjection {
    pub(crate) waiter_id: u64,
    pub(crate) state: SharedWaitStateProjection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SharedMemoryProjection {
    pub(crate) memory: LinearMemoryProjection,
    pub(crate) wait_queues: Vec<SharedWaitQueueProjection>,
    pub(crate) waiters: Vec<SharedWaiterProjection>,
    pub(crate) next_waiter_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct SharedMemoryProjectionParts {
    pub(crate) memory: LinearMemoryProjectionParts,
    pub(crate) wait_queues: Vec<SharedWaitQueueProjection>,
    pub(crate) waiters: Vec<SharedWaiterProjection>,
    pub(crate) next_waiter_id: u64,
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct SharedProtocolRegistration {
    pub(crate) wait: SharedWaitRegistration,
    pub(crate) ticket: (u64, usize),
    pub(crate) before: SharedMemoryProjection,
    pub(crate) after: SharedMemoryProjection,
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct SharedNotifyProtocolResult {
    pub(crate) woken: u32,
    pub(crate) wake_tokens: Vec<(u64, usize)>,
    pub(crate) before: SharedMemoryProjection,
    pub(crate) after: SharedMemoryProjection,
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct SharedAtomicProtocolResult<T> {
    pub(crate) result: T,
    pub(crate) before: SharedMemoryProjection,
    pub(crate) after: SharedMemoryProjection,
}

impl LinearMemoryProjection {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn proof_ready(&self) -> bool {
        self.current_pages <= self.max_pages
            && (self.current_pages as usize)
                .checked_mul(PAGE_SIZE)
                .is_some_and(|len| len == self.bytes.len())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn formal_builder_parts(&self) -> LinearMemoryProjectionParts {
        LinearMemoryProjectionParts {
            bytes: self.bytes.clone(),
            current_pages: self.current_pages,
            max_pages: self.max_pages,
            shared: self.shared,
        }
    }
}

impl SharedMemoryProjection {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn proof_ready(&self) -> bool {
        if !self.memory.proof_ready() || !self.memory.shared || self.next_waiter_id == 0 {
            return false;
        }
        let mut waiter_states = HashMap::new();
        for waiter in &self.waiters {
            if waiter.waiter_id >= self.next_waiter_id {
                return false;
            }
            if waiter_states
                .insert(waiter.waiter_id, waiter.state)
                .is_some()
            {
                return false;
            }
        }

        let mut queue_addresses = HashSet::new();
        let mut queued_waiters = HashSet::new();
        for queue in &self.wait_queues {
            if !queue_addresses.insert(queue.address) {
                return false;
            }
            for waiter_id in &queue.waiter_ids {
                if !queued_waiters.insert(*waiter_id) {
                    return false;
                }
                if waiter_states.get(waiter_id) != Some(&SharedWaitStateProjection::Waiting) {
                    return false;
                }
            }
        }

        self.waiters.iter().all(|waiter| match waiter.state {
            SharedWaitStateProjection::Waiting => queued_waiters.contains(&waiter.waiter_id),
            SharedWaitStateProjection::Notified | SharedWaitStateProjection::TimedOut => {
                !queued_waiters.contains(&waiter.waiter_id)
            }
        })
    }

    #[allow(dead_code)]
    pub(crate) fn formal_builder_parts(&self) -> SharedMemoryProjectionParts {
        SharedMemoryProjectionParts {
            memory: self.memory.formal_builder_parts(),
            wait_queues: self.wait_queues.clone(),
            waiters: self.waiters.clone(),
            next_waiter_id: self.next_waiter_id,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn queue_position(&self, address: usize) -> Option<usize> {
        self.wait_queues
            .iter()
            .position(|queue| queue.address == address)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn waiter_position(&self, waiter_id: u64) -> Option<usize> {
        self.waiters
            .iter()
            .position(|waiter| waiter.waiter_id == waiter_id)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn protocol_register_wait(&self, address: usize) -> Self {
        let mut next = self.clone();
        let waiter_id = next.next_waiter_id;
        next.next_waiter_id += 1;
        match next.queue_position(address) {
            Some(index) => next.wait_queues[index].waiter_ids.push(waiter_id),
            None => next.wait_queues.push(SharedWaitQueueProjection {
                address,
                waiter_ids: vec![waiter_id],
            }),
        }
        next.wait_queues.sort_by_key(|queue| queue.address);
        next.waiters.push(SharedWaiterProjection {
            waiter_id,
            state: SharedWaitStateProjection::Waiting,
        });
        next.waiters.sort_by_key(|waiter| waiter.waiter_id);
        next
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn protocol_notify_waiters(
        &self,
        address: usize,
        count: u32,
    ) -> (Self, Vec<SharedWaiterProjection>) {
        let mut next = self.clone();
        let mut woke = Vec::new();
        if let Some(index) = next.queue_position(address) {
            let (notified_ids, remove_queue) = {
                let queue = &mut next.wait_queues[index].waiter_ids;
                let wake_count = std::cmp::min(queue.len(), count as usize);
                let notified_ids: Vec<u64> = queue.drain(0..wake_count).collect();
                (notified_ids, queue.is_empty())
            };
            for waiter_id in &notified_ids {
                if let Some(waiter_index) = next.waiter_position(*waiter_id) {
                    next.waiters[waiter_index].state = SharedWaitStateProjection::Notified;
                    woke.push(next.waiters[waiter_index].clone());
                }
            }
            if remove_queue {
                next.wait_queues.remove(index);
            }
        }
        (next, woke)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn protocol_consume_waiter(&self, waiter_id: u64) -> Self {
        let mut next = self.clone();
        if let Some(index) = next.waiter_position(waiter_id) {
            next.waiters.remove(index);
        }
        next
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn protocol_timeout_wait(&self, address: usize, waiter_id: u64) -> Self {
        let mut next = self.clone();
        if let Some(index) = next.queue_position(address) {
            let queue = &mut next.wait_queues[index].waiter_ids;
            if let Some(waiter_index) = queue.iter().position(|current| *current == waiter_id) {
                queue.remove(waiter_index);
            }
            if queue.is_empty() {
                next.wait_queues.remove(index);
            }
        }
        if let Some(index) = next.waiter_position(waiter_id) {
            next.waiters[index].state = SharedWaitStateProjection::TimedOut;
        }
        next.protocol_consume_waiter(waiter_id)
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
    shared: bool,
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
            shared,
        }
    }

    pub fn page_size(&self) -> u32 {
        self.current_pages
    }

    pub fn data_size(&self) -> usize {
        self.current_pages as usize * PAGE_SIZE
    }

    pub(crate) fn projection(&self) -> LinearMemoryProjection {
        LinearMemoryProjection {
            bytes: self.slice().to_vec(),
            current_pages: self.current_pages,
            max_pages: self.max_pages,
            shared: self.shared,
        }
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

    pub(crate) fn projection(&self) -> LinearMemoryProjection {
        self.memory.projection()
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
    waiters: HashMap<u64, SharedWaitStateProjection>,
    next_waiter_id: u64,
}

impl SharedMemoryState {
    fn projection(&self) -> SharedMemoryProjection {
        let mut wait_queues = self
            .wait_queues
            .iter()
            .map(|(address, queue)| SharedWaitQueueProjection {
                address: *address,
                waiter_ids: queue.iter().map(|waiter| waiter.id()).collect(),
            })
            .collect::<Vec<_>>();
        wait_queues.sort_by_key(|queue| queue.address);
        let mut waiters = self
            .waiters
            .iter()
            .map(|(waiter_id, state)| SharedWaiterProjection {
                waiter_id: *waiter_id,
                state: *state,
            })
            .collect::<Vec<_>>();
        waiters.sort_by_key(|waiter| waiter.waiter_id);
        SharedMemoryProjection {
            memory: self.memory.projection(),
            wait_queues,
            waiters,
            next_waiter_id: self.next_waiter_id,
        }
    }

    fn remove_waiter_from_queue(&mut self, address: usize, waiter_id: u64) {
        if let Some(queue) = self.wait_queues.get_mut(&address) {
            if let Some(index) = queue.iter().position(|waiter| waiter.id() == waiter_id) {
                queue.remove(index);
            }
            if queue.is_empty() {
                self.wait_queues.remove(&address);
            }
        }
    }

    fn timeout_wait(&mut self, address: usize, waiter_id: u64) {
        self.remove_waiter_from_queue(address, waiter_id);
        self.waiters
            .insert(waiter_id, SharedWaitStateProjection::TimedOut);
    }

    fn consume_waiter(&mut self, waiter_id: u64) {
        self.waiters.remove(&waiter_id);
    }
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
                waiters: HashMap::new(),
                next_waiter_id: 1,
            }),
        })
    }

    pub fn page_size(&self) -> u32 {
        self.state.lock().memory.page_size()
    }

    pub(crate) fn projection(&self) -> SharedMemoryProjection {
        self.state.lock().projection()
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
        vm_try!(self.atomic_store_protocol_u8(offset, value));
        VMResult::Success(())
    }

    #[inline(always)]
    pub fn atomic_store_u16(&self, offset: usize, value: u16) -> VMResult<()> {
        vm_try!(self.atomic_store_protocol_u16(offset, value));
        VMResult::Success(())
    }

    #[inline(always)]
    pub fn atomic_store_u32(&self, offset: usize, value: u32) -> VMResult<()> {
        vm_try!(self.atomic_store_protocol_u32(offset, value));
        VMResult::Success(())
    }

    #[inline(always)]
    pub fn atomic_store_u64(&self, offset: usize, value: u64) -> VMResult<()> {
        vm_try!(self.atomic_store_protocol_u64(offset, value));
        VMResult::Success(())
    }

    #[inline(always)]
    pub fn atomic_rmw_u8(&self, offset: usize, op: AtomicRmwOp, value: u8) -> VMResult<u8> {
        let protocol = vm_try!(self.atomic_rmw_protocol_u8(offset, op, value));
        VMResult::Success(protocol.result)
    }

    #[inline(always)]
    pub fn atomic_rmw_u16(&self, offset: usize, op: AtomicRmwOp, value: u16) -> VMResult<u16> {
        let protocol = vm_try!(self.atomic_rmw_protocol_u16(offset, op, value));
        VMResult::Success(protocol.result)
    }

    #[inline(always)]
    pub fn atomic_rmw_u32(&self, offset: usize, op: AtomicRmwOp, value: u32) -> VMResult<u32> {
        let protocol = vm_try!(self.atomic_rmw_protocol_u32(offset, op, value));
        VMResult::Success(protocol.result)
    }

    #[inline(always)]
    pub fn atomic_rmw_u64(&self, offset: usize, op: AtomicRmwOp, value: u64) -> VMResult<u64> {
        let protocol = vm_try!(self.atomic_rmw_protocol_u64(offset, op, value));
        VMResult::Success(protocol.result)
    }

    #[inline(always)]
    pub fn atomic_cmpxchg_u8(&self, offset: usize, expected: u8, value: u8) -> VMResult<u8> {
        let protocol = vm_try!(self.atomic_cmpxchg_protocol_u8(offset, expected, value));
        VMResult::Success(protocol.result)
    }

    #[inline(always)]
    pub fn atomic_cmpxchg_u16(&self, offset: usize, expected: u16, value: u16) -> VMResult<u16> {
        let protocol = vm_try!(self.atomic_cmpxchg_protocol_u16(offset, expected, value));
        VMResult::Success(protocol.result)
    }

    #[inline(always)]
    pub fn atomic_cmpxchg_u32(&self, offset: usize, expected: u32, value: u32) -> VMResult<u32> {
        let protocol = vm_try!(self.atomic_cmpxchg_protocol_u32(offset, expected, value));
        VMResult::Success(protocol.result)
    }

    #[inline(always)]
    pub fn atomic_cmpxchg_u64(&self, offset: usize, expected: u64, value: u64) -> VMResult<u64> {
        let protocol = vm_try!(self.atomic_cmpxchg_protocol_u64(offset, expected, value));
        VMResult::Success(protocol.result)
    }

    #[inline(always)]
    pub fn atomic_fence(&self) {
        let _state = self.state.lock();
    }

    pub(crate) fn register_wait32_protocol(
        &self,
        offset: usize,
        expected: u32,
    ) -> VMResult<Option<SharedProtocolRegistration>> {
        let mut state = self.state.lock();
        let before = state.projection();
        vm_try!(ensure_atomic_alignment(offset, 4));
        let current = vm_try!(state.memory.atomic_load_u32(offset));
        if current != expected {
            return VMResult::Success(None);
        }
        let waiter = Arc::new(SharedWaiter::new(state.next_waiter_id));
        state.next_waiter_id += 1;
        state
            .waiters
            .insert(waiter.id(), SharedWaitStateProjection::Waiting);
        state
            .wait_queues
            .entry(offset)
            .or_default()
            .push_back(waiter.clone());
        let after = state.projection();
        VMResult::Success(Some(SharedProtocolRegistration {
            ticket: (waiter.id(), offset),
            wait: SharedWaitRegistration {
                address: offset,
                waiter,
            },
            before,
            after,
        }))
    }

    pub fn register_wait32(&self, offset: usize, expected: u32) -> VMResult<AtomicWaitResult> {
        match vm_try!(self.register_wait32_protocol(offset, expected)) {
            Some(protocol) => VMResult::Success(AtomicWaitResult::Pending(protocol.wait)),
            None => VMResult::Success(AtomicWaitResult::NotEqual),
        }
    }

    pub(crate) fn register_wait64_protocol(
        &self,
        offset: usize,
        expected: u64,
    ) -> VMResult<Option<SharedProtocolRegistration>> {
        let mut state = self.state.lock();
        let before = state.projection();
        vm_try!(ensure_atomic_alignment(offset, 8));
        let current = vm_try!(state.memory.atomic_load_u64(offset));
        if current != expected {
            return VMResult::Success(None);
        }
        let waiter = Arc::new(SharedWaiter::new(state.next_waiter_id));
        state.next_waiter_id += 1;
        state
            .waiters
            .insert(waiter.id(), SharedWaitStateProjection::Waiting);
        state
            .wait_queues
            .entry(offset)
            .or_default()
            .push_back(waiter.clone());
        let after = state.projection();
        VMResult::Success(Some(SharedProtocolRegistration {
            ticket: (waiter.id(), offset),
            wait: SharedWaitRegistration {
                address: offset,
                waiter,
            },
            before,
            after,
        }))
    }

    pub fn register_wait64(&self, offset: usize, expected: u64) -> VMResult<AtomicWaitResult> {
        match vm_try!(self.register_wait64_protocol(offset, expected)) {
            Some(protocol) => VMResult::Success(AtomicWaitResult::Pending(protocol.wait)),
            None => VMResult::Success(AtomicWaitResult::NotEqual),
        }
    }

    pub(crate) fn notify_waiters_protocol(
        &self,
        offset: usize,
        count: u32,
    ) -> VMResult<SharedNotifyProtocolResult> {
        let mut state = self.state.lock();
        let before = state.projection();
        vm_try!(state.memory.atomic_load_u32(offset));
        let mut wake = Vec::new();
        let mut notified_ids = Vec::new();
        let mut remaining = count;
        let mut remove_queue = false;
        if let Some(queue) = state.wait_queues.get_mut(&offset) {
            while remaining != 0 {
                let Some(waiter) = queue.pop_front() else {
                    break;
                };
                if waiter.try_mark_notified() {
                    notified_ids.push(waiter.id());
                    wake.push(waiter);
                    remaining -= 1;
                }
            }
            queue.retain(|waiter| waiter.is_waiting());
            remove_queue = queue.is_empty();
        }
        for waiter_id in &notified_ids {
            state
                .waiters
                .insert(*waiter_id, SharedWaitStateProjection::Notified);
        }
        if remove_queue {
            state.wait_queues.remove(&offset);
        }
        let wake_tokens = notified_ids
            .iter()
            .copied()
            .map(|waiter_id| (waiter_id, offset))
            .collect::<Vec<_>>();
        let woken = wake_tokens.len() as u32;
        let after = state.projection();
        drop(state);
        for waiter in wake {
            waiter.wake();
        }
        VMResult::Success(SharedNotifyProtocolResult {
            woken,
            wake_tokens,
            before,
            after,
        })
    }

    pub fn notify_waiters(&self, offset: usize, count: u32) -> VMResult<u32> {
        let protocol = vm_try!(self.notify_waiters_protocol(offset, count));
        VMResult::Success(protocol.woken)
    }

    fn consume_wake(&self, address: usize, waiter_id: u64) -> SharedAtomicProtocolResult<i32> {
        let mut state = self.state.lock();
        let before = state.projection();
        state.remove_waiter_from_queue(address, waiter_id);
        state.consume_waiter(waiter_id);
        let after = state.projection();
        SharedAtomicProtocolResult {
            result: 0,
            before,
            after,
        }
    }

    fn consume_timed_out(&self, address: usize, waiter_id: u64) -> SharedAtomicProtocolResult<i32> {
        let mut state = self.state.lock();
        let before = state.projection();
        state.timeout_wait(address, waiter_id);
        state.consume_waiter(waiter_id);
        let after = state.projection();
        SharedAtomicProtocolResult {
            result: 2,
            before,
            after,
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

macro_rules! define_shared_atomic_store_protocol {
    ($protocol:ident, $public:ident, $ty:ty) => {
        #[cfg_attr(not(test), allow(dead_code))]
        pub(crate) fn $protocol(
            &self,
            offset: usize,
            value: $ty,
        ) -> VMResult<SharedAtomicProtocolResult<()>> {
            let mut state = self.state.lock();
            let before = state.projection();
            vm_try!(state.memory.$public(offset, value));
            let after = state.projection();
            VMResult::Success(SharedAtomicProtocolResult {
                result: (),
                before,
                after,
            })
        }
    };
}

macro_rules! define_shared_atomic_rmw_protocol {
    ($protocol:ident, $public:ident, $ty:ty) => {
        #[cfg_attr(not(test), allow(dead_code))]
        pub(crate) fn $protocol(
            &self,
            offset: usize,
            op: AtomicRmwOp,
            value: $ty,
        ) -> VMResult<SharedAtomicProtocolResult<$ty>> {
            let mut state = self.state.lock();
            let before = state.projection();
            let result = vm_try!(state.memory.$public(offset, op, value));
            let after = state.projection();
            VMResult::Success(SharedAtomicProtocolResult {
                result,
                before,
                after,
            })
        }
    };
}

macro_rules! define_shared_atomic_cmpxchg_protocol {
    ($protocol:ident, $public:ident, $ty:ty) => {
        #[cfg_attr(not(test), allow(dead_code))]
        pub(crate) fn $protocol(
            &self,
            offset: usize,
            expected: $ty,
            value: $ty,
        ) -> VMResult<SharedAtomicProtocolResult<$ty>> {
            let mut state = self.state.lock();
            let before = state.projection();
            let result = vm_try!(state.memory.$public(offset, expected, value));
            let after = state.projection();
            VMResult::Success(SharedAtomicProtocolResult {
                result,
                before,
                after,
            })
        }
    };
}

impl SharedMemoryObject {
    define_shared_atomic_store_protocol!(atomic_store_protocol_u8, atomic_store_u8, u8);
    define_shared_atomic_store_protocol!(atomic_store_protocol_u16, atomic_store_u16, u16);
    define_shared_atomic_store_protocol!(atomic_store_protocol_u32, atomic_store_u32, u32);
    define_shared_atomic_store_protocol!(atomic_store_protocol_u64, atomic_store_u64, u64);

    define_shared_atomic_rmw_protocol!(atomic_rmw_protocol_u8, atomic_rmw_u8, u8);
    define_shared_atomic_rmw_protocol!(atomic_rmw_protocol_u16, atomic_rmw_u16, u16);
    define_shared_atomic_rmw_protocol!(atomic_rmw_protocol_u32, atomic_rmw_u32, u32);
    define_shared_atomic_rmw_protocol!(atomic_rmw_protocol_u64, atomic_rmw_u64, u64);

    define_shared_atomic_cmpxchg_protocol!(atomic_cmpxchg_protocol_u8, atomic_cmpxchg_u8, u8);
    define_shared_atomic_cmpxchg_protocol!(atomic_cmpxchg_protocol_u16, atomic_cmpxchg_u16, u16);
    define_shared_atomic_cmpxchg_protocol!(atomic_cmpxchg_protocol_u32, atomic_cmpxchg_u32, u32);
    define_shared_atomic_cmpxchg_protocol!(atomic_cmpxchg_protocol_u64, atomic_cmpxchg_u64, u64);
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::future::poll_fn;
    use std::{sync::Arc, time::Duration};

    async fn wait_result(
        shared: Arc<SharedMemoryObject>,
        wait: SharedWaitRegistration,
        timeout_ns: i64,
    ) -> i32 {
        if timeout_ns < 0 {
            poll_fn(|cx| wait.poll_wait(cx)).await;
            wait.finish_notified(&shared)
        } else {
            let sleep = tokio::time::sleep(Duration::from_nanos(timeout_ns as u64));
            tokio::pin!(sleep);
            tokio::select! {
                _ = poll_fn(|cx| wait.poll_wait(cx)) => wait.finish_notified(&shared),
                _ = &mut sleep => wait.finish_timeout(&shared),
            }
        }
    }

    #[test]
    fn memory_write_copy_fill_and_grow_match_linear_model_bytes() {
        let mut memory = Memory::new(1, 3);

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

        let projection = memory.projection();
        assert_eq!(projection.current_pages, 2);
        assert_eq!(projection.max_pages, 3);
        assert!(!projection.shared);
        assert_eq!(&projection.bytes[0..16], &memory.slice()[0..16]);
    }

    #[tokio::test]
    async fn shared_wait_queue_internal_state_tracks_notify_and_timeout_cleanup() {
        let shared = SharedMemoryObject::new(1, 1);
        assert!(matches!(
            shared.atomic_store_u32(0, 7),
            VMResult::Success(())
        ));

        let mut expected = shared.projection();
        let first = match shared.register_wait32(0, 7).unwrap() {
            AtomicWaitResult::Pending(wait) => wait,
            AtomicWaitResult::NotEqual => panic!("expected waiting registration"),
        };
        let first_waiter_id = first.waiter.id();
        expected = expected.protocol_register_wait(0);
        let second = match shared.register_wait32(0, 7).unwrap() {
            AtomicWaitResult::Pending(wait) => wait,
            AtomicWaitResult::NotEqual => panic!("expected waiting registration"),
        };
        let second_waiter_id = second.waiter.id();
        expected = expected.protocol_register_wait(0);

        {
            let state = shared.state.lock();
            let queue = state.wait_queues.get(&0).expect("queue must exist");
            assert_eq!(queue.len(), 2);
            assert!(queue.iter().all(|waiter| waiter.is_waiting()));
            assert_eq!(state.next_waiter_id, 3);
            assert_eq!(
                state.waiters.get(&first_waiter_id),
                Some(&SharedWaitStateProjection::Waiting)
            );
            assert_eq!(
                state.waiters.get(&second_waiter_id),
                Some(&SharedWaitStateProjection::Waiting)
            );
        }

        assert_eq!(shared.notify_waiters(0, 1).unwrap(), 1);
        let (expected_after_notify, _) = expected.protocol_notify_waiters(0, 1);
        assert_eq!(shared.projection(), expected_after_notify);
        {
            let state = shared.state.lock();
            assert_eq!(
                state.waiters.get(&first_waiter_id),
                Some(&SharedWaitStateProjection::Notified)
            );
            assert_eq!(
                state.waiters.get(&second_waiter_id),
                Some(&SharedWaitStateProjection::Waiting)
            );
        }
        assert_eq!(wait_result(shared.clone(), first, -1).await, 0);
        let expected_after_first = expected_after_notify.protocol_consume_waiter(first_waiter_id);
        assert_eq!(shared.projection(), expected_after_first);

        {
            let state = shared.state.lock();
            let queue = state.wait_queues.get(&0).expect("one waiter should remain");
            assert_eq!(queue.len(), 1);
            assert_eq!(queue.front().unwrap().id(), second_waiter_id);
            assert!(queue.front().unwrap().is_waiting());
            assert!(!state.waiters.contains_key(&first_waiter_id));
            assert_eq!(
                state.waiters.get(&second_waiter_id),
                Some(&SharedWaitStateProjection::Waiting)
            );
        }

        assert_eq!(wait_result(shared.clone(), second, 0).await, 2);
        let expected_after_second = expected_after_first.protocol_timeout_wait(0, second_waiter_id);
        assert_eq!(shared.projection(), expected_after_second);

        {
            let state = shared.state.lock();
            assert!(!state.wait_queues.contains_key(&0));
            assert!(state.waiters.is_empty());
        }
        assert_eq!(shared.notify_waiters(0, 1).unwrap(), 0);
    }

    #[tokio::test]
    async fn shared_wait_queue_rejects_mismatch_and_notifies_fifo_up_to_count() {
        let shared = SharedMemoryObject::new(1, 1);
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
        assert_eq!(wait_result(shared.clone(), first, 0).await, 0);
        assert_eq!(wait_result(shared.clone(), second, 0).await, 0);

        {
            let state = shared.state.lock();
            let queue = state.wait_queues.get(&0).expect("one waiter should remain");
            assert_eq!(queue.len(), 1);
            assert_eq!(queue.front().unwrap().id(), third.waiter.id());
        }

        assert_eq!(shared.notify_waiters(0, 10).unwrap(), 1);
        assert_eq!(wait_result(shared.clone(), third, 0).await, 0);
        assert_eq!(shared.notify_waiters(0, 1).unwrap(), 0);
    }

    #[test]
    fn shared_memory_projection_tracks_queue_ids_and_waiter_states() {
        let shared = SharedMemoryObject::new(1, 1);
        assert!(matches!(
            shared.atomic_store_u32(0, 21),
            VMResult::Success(())
        ));

        let mut expected = shared.projection();
        let first = match shared.register_wait32(0, 21).unwrap() {
            AtomicWaitResult::Pending(wait) => wait,
            AtomicWaitResult::NotEqual => panic!("expected waiting registration"),
        };
        expected = expected.protocol_register_wait(0);
        assert_eq!(shared.projection(), expected);
        let second = match shared.register_wait32(0, 21).unwrap() {
            AtomicWaitResult::Pending(wait) => wait,
            AtomicWaitResult::NotEqual => panic!("expected waiting registration"),
        };
        expected = expected.protocol_register_wait(0);

        let before = shared.projection();
        assert_eq!(before, expected);
        assert!(before.proof_ready());
        assert!(before.memory.proof_ready());
        let before_parts = before.memory.formal_builder_parts();
        assert_eq!(before_parts.current_pages, before.memory.current_pages);
        assert_eq!(before_parts.max_pages, before.memory.max_pages);
        assert!(before.memory.shared);
        assert_eq!(before.next_waiter_id, 3);
        assert_eq!(before.wait_queues.len(), 1);
        assert_eq!(before.wait_queues[0].address, 0);
        assert_eq!(
            before.wait_queues[0].waiter_ids,
            vec![first.waiter.id(), second.waiter.id()]
        );
        assert!(before
            .waiters
            .iter()
            .all(|waiter| waiter.state == SharedWaitStateProjection::Waiting));

        assert_eq!(shared.notify_waiters(0, 1).unwrap(), 1);
        let (after_notify, woke) = expected.protocol_notify_waiters(0, 1);
        let after = shared.projection();
        assert_eq!(after, after_notify);
        assert!(after.proof_ready());
        assert_eq!(woke.len(), 1);
        assert_eq!(woke[0].waiter_id, first.waiter.id());
        assert_eq!(after.wait_queues[0].waiter_ids, vec![second.waiter.id()]);
        assert_eq!(
            after
                .waiters
                .iter()
                .find(|waiter| waiter.waiter_id == first.waiter.id())
                .unwrap()
                .state,
            SharedWaitStateProjection::Notified
        );
        assert_eq!(
            after
                .waiters
                .iter()
                .find(|waiter| waiter.waiter_id == second.waiter.id())
                .unwrap()
                .state,
            SharedWaitStateProjection::Waiting
        );

        let mut unsorted = after.clone();
        unsorted.wait_queues.reverse();
        unsorted.waiters.reverse();
        assert!(unsorted.proof_ready());

        unsorted.wait_queues[0].waiter_ids.push(u64::MAX);
        assert!(!unsorted.proof_ready());
    }

    #[test]
    fn shared_memory_projection_parts_capture_protocol_fields() {
        let shared = SharedMemoryObject::new(1, 1);
        assert!(matches!(
            shared.atomic_store_u32(0, 21),
            VMResult::Success(())
        ));

        let registration = shared
            .register_wait32_protocol(0, 21)
            .unwrap()
            .expect("expected waiting registration");
        let projection = shared.projection();
        let parts = projection.formal_builder_parts();

        assert!(projection.proof_ready());
        assert_eq!(parts.memory.bytes, projection.memory.bytes);
        assert_eq!(parts.memory.current_pages, projection.memory.current_pages);
        assert_eq!(parts.memory.max_pages, projection.memory.max_pages);
        assert_eq!(parts.memory.shared, projection.memory.shared);
        assert_eq!(parts.wait_queues, projection.wait_queues);
        assert_eq!(parts.waiters, projection.waiters);
        assert_eq!(parts.next_waiter_id, projection.next_waiter_id);
        assert_eq!(registration.after, projection);
    }

    #[test]
    fn shared_memory_proof_ready_rejects_protocol_violations() {
        let shared = SharedMemoryObject::new(1, 1);
        assert!(matches!(
            shared.atomic_store_u32(0, 33),
            VMResult::Success(())
        ));
        let _registration = shared
            .register_wait32_protocol(0, 33)
            .unwrap()
            .expect("expected waiting registration");

        let projection = shared.projection();
        assert!(projection.proof_ready());

        let mut duplicate_waiter = projection.clone();
        duplicate_waiter
            .waiters
            .push(duplicate_waiter.waiters[0].clone());
        assert!(!duplicate_waiter.proof_ready());

        let mut unqueued_waiting = projection.clone();
        unqueued_waiting.wait_queues.clear();
        assert!(!unqueued_waiting.proof_ready());

        let mut queued_notified = projection.clone();
        queued_notified.waiters[0].state = SharedWaitStateProjection::Notified;
        assert!(!queued_notified.proof_ready());

        let mut local_like = projection.clone();
        local_like.memory.shared = false;
        assert!(!local_like.proof_ready());
    }

    #[test]
    fn shared_wait_protocol_wrappers_track_before_after_states() {
        let shared = SharedMemoryObject::new(1, 1);
        assert!(matches!(
            shared.atomic_store_u32(0, 7),
            VMResult::Success(())
        ));
        assert!(shared.register_wait32_protocol(0, 9).unwrap().is_none());

        let first = shared
            .register_wait32_protocol(0, 7)
            .unwrap()
            .expect("expected first waiting registration");
        assert_eq!(first.ticket, (1, 0));
        assert_eq!(first.after, first.before.protocol_register_wait(0));

        let second = shared
            .register_wait32_protocol(0, 7)
            .unwrap()
            .expect("expected second waiting registration");
        assert_eq!(second.ticket, (2, 0));
        assert_eq!(second.before, first.after);
        assert_eq!(second.after, second.before.protocol_register_wait(0));

        let notify = shared.notify_waiters_protocol(0, 1).unwrap();
        let (expected_after_notify, woke) = notify.before.protocol_notify_waiters(0, 1);
        assert_eq!(notify.before, second.after);
        assert_eq!(notify.after, expected_after_notify);
        assert_eq!(notify.woken, 1);
        assert_eq!(notify.wake_tokens, vec![(woke[0].waiter_id, 0)]);

        let first_waiter_id = first.ticket.0;
        let finish_notified = first.wait.finish_notified_protocol(&shared);
        assert_eq!(finish_notified.before, notify.after);
        assert_eq!(
            finish_notified.after,
            finish_notified
                .before
                .protocol_consume_waiter(first_waiter_id)
        );
        assert_eq!(finish_notified.result, 0);

        let second_waiter_id = second.ticket.0;
        let finish_timeout = second.wait.finish_timeout_protocol(&shared);
        assert_eq!(finish_timeout.before, finish_notified.after);
        assert_eq!(
            finish_timeout.after,
            finish_timeout
                .before
                .protocol_timeout_wait(0, second_waiter_id)
        );
        assert_eq!(finish_timeout.result, 2);
        assert_eq!(shared.projection(), finish_timeout.after);
    }

    #[test]
    fn shared_atomic_protocol_wrappers_cover_all_widths() {
        let shared = SharedMemoryObject::new(1, 1);

        let store_u8 = shared.atomic_store_protocol_u8(0, 0x10).unwrap();
        let store_u16 = shared.atomic_store_protocol_u16(2, 0x1122).unwrap();
        let store_u32 = shared.atomic_store_protocol_u32(4, 0x3344_5566).unwrap();
        let store_u64 = shared
            .atomic_store_protocol_u64(8, 0x0102_0304_0506_0708)
            .unwrap();
        assert!(store_u8.after.proof_ready());
        assert_eq!(shared.atomic_load_u8(0).unwrap(), 0x10);
        assert_eq!(shared.atomic_load_u16(2).unwrap(), 0x1122);
        assert_eq!(shared.atomic_load_u32(4).unwrap(), 0x3344_5566);
        assert_eq!(shared.atomic_load_u64(8).unwrap(), 0x0102_0304_0506_0708);
        assert_eq!(store_u16.after, store_u32.before);
        assert_eq!(store_u32.after, store_u64.before);

        let rmw_u8 = shared
            .atomic_rmw_protocol_u8(0, AtomicRmwOp::Add, 1)
            .unwrap();
        let rmw_u16 = shared
            .atomic_rmw_protocol_u16(2, AtomicRmwOp::Xor, 0x00ff)
            .unwrap();
        let rmw_u32 = shared
            .atomic_rmw_protocol_u32(4, AtomicRmwOp::Add, 1)
            .unwrap();
        let rmw_u64 = shared
            .atomic_rmw_protocol_u64(8, AtomicRmwOp::Xchg, 0xf0e0_d0c0_b0a0_9080)
            .unwrap();
        assert_eq!(rmw_u8.result, 0x10);
        assert_eq!(rmw_u16.result, 0x1122);
        assert_eq!(rmw_u32.result, 0x3344_5566);
        assert_eq!(rmw_u64.result, 0x0102_0304_0506_0708);
        assert_eq!(shared.atomic_load_u8(0).unwrap(), 0x11);
        assert_eq!(shared.atomic_load_u16(2).unwrap(), 0x11dd);
        assert_eq!(shared.atomic_load_u32(4).unwrap(), 0x3344_5567);
        assert_eq!(shared.atomic_load_u64(8).unwrap(), 0xf0e0_d0c0_b0a0_9080);

        let cmpxchg_u8 = shared.atomic_cmpxchg_protocol_u8(0, 0x11, 0xaa).unwrap();
        let cmpxchg_u16 = shared
            .atomic_cmpxchg_protocol_u16(2, 0xffff, 0xbeef)
            .unwrap();
        let cmpxchg_u32 = shared
            .atomic_cmpxchg_protocol_u32(4, 0x3344_5567, 0x4455_6677)
            .unwrap();
        let cmpxchg_u64 = shared
            .atomic_cmpxchg_protocol_u64(8, 0xf0e0_d0c0_b0a0_9080, 0x8877_6655_4433_2211)
            .unwrap();
        assert_eq!(cmpxchg_u8.result, 0x11);
        assert_eq!(cmpxchg_u16.result, 0x11dd);
        assert_eq!(cmpxchg_u32.result, 0x3344_5567);
        assert_eq!(cmpxchg_u64.result, 0xf0e0_d0c0_b0a0_9080);
        assert_eq!(shared.atomic_load_u8(0).unwrap(), 0xaa);
        assert_eq!(shared.atomic_load_u16(2).unwrap(), 0x11dd);
        assert_eq!(shared.atomic_load_u32(4).unwrap(), 0x4455_6677);
        assert_eq!(shared.atomic_load_u64(8).unwrap(), 0x8877_6655_4433_2211);
        assert!(cmpxchg_u64.after.proof_ready());
    }
}
