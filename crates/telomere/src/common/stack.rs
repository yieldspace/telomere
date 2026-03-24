#![allow(private_interfaces)]

mod footer;
mod ref_scan;

#[cfg(feature = "simd")]
use wide::{f32x4, f64x2, i16x8, i32x4, i64x2, i8x16, u16x8, u32x4, u64x2, u8x16};

use crate::VMResult;
use std::{fmt::Debug, ptr::NonNull};

use super::{
    memory::trusted_copy_from_slice,
    object_ref::ObjectRef,
    store::{
        FunctionInstanceData, InstanceId, InstanceMemorySlot, LocalMemoryId, MemoryHandle,
        PrecomputedFunctionReturnSite, SharedMemoryId,
    },
    FrameLayoutHeader, Instr, ReturnShape, StablePc, StoreInner, UnwindSiteMetadata,
};
pub(crate) use footer::{CachedMemoryKind, CallFrameCache, CallStackInfo};
#[inline(always)]
fn trusted_write_u32(dst: &mut [u8], value: u32) {
    debug_assert_eq!(dst.len(), 4);
    unsafe {
        dst.as_mut_ptr()
            .cast::<u32>()
            .write_unaligned(value.to_le());
    }
}

#[inline(always)]
fn trusted_write_u64(dst: &mut [u8], value: u64) {
    debug_assert_eq!(dst.len(), 8);
    unsafe {
        dst.as_mut_ptr()
            .cast::<u64>()
            .write_unaligned(value.to_le());
    }
}

#[inline(always)]
fn trusted_write_u128(dst: &mut [u8], value: u128) {
    debug_assert_eq!(dst.len(), 16);
    unsafe {
        dst.as_mut_ptr()
            .cast::<u128>()
            .write_unaligned(value.to_le());
    }
}

#[inline(always)]
fn trusted_read_u32(src: &[u8]) -> u32 {
    debug_assert_eq!(src.len(), 4);
    unsafe { u32::from_le(src.as_ptr().cast::<u32>().read_unaligned()) }
}

#[inline(always)]
fn trusted_read_u64(src: &[u8]) -> u64 {
    debug_assert_eq!(src.len(), 8);
    unsafe { u64::from_le(src.as_ptr().cast::<u64>().read_unaligned()) }
}

#[inline(always)]
fn trusted_read_u128(src: &[u8]) -> u128 {
    debug_assert_eq!(src.len(), 16);
    unsafe { u128::from_le(src.as_ptr().cast::<u128>().read_unaligned()) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CachedOperandWidth {
    None,
    Four,
    Eight,
}

impl CachedOperandWidth {
    #[inline(always)]
    const fn bytes(self) -> usize {
        match self {
            Self::None => 0,
            Self::Four => 4,
            Self::Eight => 8,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct OperandCache {
    width: CachedOperandWidth,
    bits: u64,
}

impl OperandCache {
    const EMPTY: Self = Self {
        width: CachedOperandWidth::None,
        bits: 0,
    };

    #[inline(always)]
    fn clear(&mut self) {
        *self = Self::EMPTY;
    }
}

pub(crate) trait LaneType
where
    Self: Sized,
{
    type BaseType;
    const LANE_SIZE: usize = std::mem::size_of::<Self>() / std::mem::size_of::<Self::BaseType>();
}
macro_rules! impl_lane_type {
    ($target: ty,$base: ty) => {
        impl LaneType for $target {
            type BaseType = $base;
        }
    };
}
#[cfg(feature = "simd")]
impl_lane_type!(f32x4, f32);
#[cfg(feature = "simd")]
impl_lane_type!(f64x2, f64);
#[cfg(feature = "simd")]
impl_lane_type!(i8x16, i8);
#[cfg(feature = "simd")]
impl_lane_type!(i16x8, i16);
#[cfg(feature = "simd")]
impl_lane_type!(i32x4, i32);
#[cfg(feature = "simd")]
impl_lane_type!(i64x2, i64);
#[cfg(feature = "simd")]
impl_lane_type!(u8x16, u8);
#[cfg(feature = "simd")]
impl_lane_type!(u16x8, u16);
#[cfg(feature = "simd")]
impl_lane_type!(u32x4, u32);
#[cfg(feature = "simd")]
impl_lane_type!(u64x2, u64);

pub struct Stack {
    memory: Box<[u8]>,
    top: usize,
    cache: OperandCache,
}
impl Debug for Stack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Stack(top={},committed_top={},cache={:?},memory={:?})",
            self.top,
            self.committed_top(),
            self.cache,
            &self.memory[0..self.committed_top()]
        )
    }
}
pub trait IntoCallFrameCache {
    fn into_call_frame_cache(self, runtime: &StoreInner) -> CallFrameCache;
}

impl IntoCallFrameCache for CallFrameCache {
    fn into_call_frame_cache(self, _runtime: &StoreInner) -> CallFrameCache {
        self
    }
}

impl IntoCallFrameCache for ObjectRef {
    fn into_call_frame_cache(self, runtime: &StoreInner) -> CallFrameCache {
        let func = runtime.get_func(self);
        let instance = runtime.instance(func.instance);
        let memory0 = instance
            .memory_slots
            .first()
            .copied()
            .unwrap_or(InstanceMemorySlot::None)
            .handle();
        CallFrameCache::from_parts(self, func, memory0)
    }
}
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct LocalReference {
    pub local_top: usize,
    pub frame_bytes: u32,
    pub layout: Option<NonNull<FrameLayoutHeader>>,
}

impl LocalReference {
    #[inline(always)]
    pub const fn empty() -> Self {
        Self {
            local_top: 0,
            frame_bytes: 0,
            layout: None,
        }
    }

    #[inline(always)]
    pub const fn from_raw(local_top: usize, frame_bytes: u32) -> Self {
        Self {
            local_top,
            frame_bytes,
            layout: None,
        }
    }

    #[inline(always)]
    pub fn from_layout(local_top: usize, layout: &FrameLayoutHeader) -> Self {
        Self {
            local_top,
            frame_bytes: layout.fixed_frame_bytes,
            layout: Some(NonNull::from(layout)),
        }
    }

    #[inline(always)]
    pub const fn has_call_stack_info(self) -> bool {
        self.frame_bytes as usize >= std::mem::size_of::<CallStackInfo>()
    }
}
impl Stack {
    pub fn new(size: usize) -> Self {
        let vec = vec![0; size];
        Stack {
            memory: vec.into_boxed_slice(),
            top: 0,
            cache: OperandCache::EMPTY,
        }
    }
    #[inline(always)]
    fn committed_top(&self) -> usize {
        self.top - self.cache.width.bytes()
    }
    #[inline(always)]
    fn flush_cached_operands(&mut self) {
        match self.cache.width {
            CachedOperandWidth::None => {}
            CachedOperandWidth::Four => {
                let start = self.top - 4;
                trusted_write_u32(&mut self.memory[start..self.top], self.cache.bits as u32);
            }
            CachedOperandWidth::Eight => {
                let start = self.top - 8;
                trusted_write_u64(&mut self.memory[start..self.top], self.cache.bits);
            }
        }
        self.cache.clear();
    }
    #[inline(always)]
    fn set_cached_operand(&mut self, width: CachedOperandWidth, bits: u64) {
        self.cache = OperandCache { width, bits };
    }
    #[inline(always)]
    fn peek_cached_operand(&self, width: CachedOperandWidth) -> Option<u64> {
        (self.cache.width == width).then_some(self.cache.bits)
    }
    #[inline(always)]
    fn checked_new_top(&self, n: usize) -> VMResult<usize> {
        let new_top = vm_try!(VMResult::from_option(self.top.checked_add(n), || {
            VMResult::StackOverflow
        }));
        if new_top > self.memory.len() {
            return VMResult::StackOverflow;
        }
        VMResult::Success(new_top)
    }
    #[inline(always)]
    fn push_cached_u32(&mut self, v: u32) -> VMResult<()> {
        let new_top = vm_try!(self.checked_new_top(4));
        if self.cache.width != CachedOperandWidth::None {
            self.flush_cached_operands();
        }
        self.top = new_top;
        self.set_cached_operand(CachedOperandWidth::Four, u64::from(v));
        VMResult::Success(())
    }
    #[inline(always)]
    fn push_cached_u64(&mut self, v: u64) -> VMResult<()> {
        let new_top = vm_try!(self.checked_new_top(8));
        if self.cache.width != CachedOperandWidth::None {
            self.flush_cached_operands();
        }
        self.top = new_top;
        self.set_cached_operand(CachedOperandWidth::Eight, v);
        VMResult::Success(())
    }
    fn add_top(&mut self, n: usize) -> VMResult<()> {
        self.top = vm_try!(VMResult::from_option(self.top.checked_add(n), || {
            VMResult::StackOverflow
        }));
        VMResult::Success(())
    }
    fn sub_top(&mut self, n: usize) {
        self.top -= n;
    }
    fn get_memory(&mut self, n: usize) -> VMResult<&mut [u8]> {
        let last = vm_try!(VMResult::from_option(self.top.checked_add(n), || {
            VMResult::StackOverflow
        }));
        VMResult::Success(vm_try!(VMResult::from_option(
            self.memory.get_mut(self.top..last),
            || VMResult::StackOverflow
        )))
    }
    pub fn push_u8_array<const N: usize>(&mut self, v: [u8; N]) -> VMResult<()> {
        self.flush_cached_operands();
        trusted_copy_from_slice(vm_try!(self.get_memory(N)), &v);
        self.add_top(N)
    }

    pub fn push_slice(&mut self, v: &[u8]) -> VMResult<()> {
        self.flush_cached_operands();
        trusted_copy_from_slice(vm_try!(self.get_memory(v.len())), v);
        self.add_top(v.len())
    }
    pub fn pop_u8_array<const N: usize>(&mut self) -> [u8; N] {
        self.flush_cached_operands();
        self.sub_top(N);
        let mut arr = [0u8; N];
        trusted_copy_from_slice(&mut arr, &self.memory[self.top..self.top + N]);
        arr
    }
    pub fn pop_u8_array_generic<const N: usize>(&mut self, n: usize) -> [u8; N] {
        self.flush_cached_operands();
        self.sub_top(n);

        let mut arr = [0u8; N];
        trusted_copy_from_slice(&mut arr, &self.memory[self.top..self.top + N]);
        arr
    }
    pub fn drop(&mut self, n: usize) -> &[u8] {
        self.flush_cached_operands();
        self.sub_top(n);

        (&self.memory[self.top..self.top + n]) as _
    }
    #[inline(always)]
    pub fn push_u32(&mut self, v: u32) -> VMResult<()> {
        self.push_cached_u32(v)
    }
    #[inline(always)]
    pub fn pop_u32(&mut self) -> u32 {
        if self.cache.width == CachedOperandWidth::Four {
            let bits = self.cache.bits as u32;
            self.top -= 4;
            self.cache.clear();
            return bits;
        }
        self.sub_top(4);
        trusted_read_u32(&self.memory[self.top..self.top + 4])
    }
    #[inline(always)]
    pub fn push_u64(&mut self, v: u64) -> VMResult<()> {
        self.push_cached_u64(v)
    }

    pub fn push_u128(&mut self, v: u128) -> VMResult<()> {
        self.flush_cached_operands();
        trusted_write_u128(vm_try!(self.get_memory(16)), v);
        self.add_top(16)
    }
    pub fn pop_u128(&mut self) -> u128 {
        self.flush_cached_operands();
        self.sub_top(16);
        trusted_read_u128(&self.memory[self.top..self.top + 16])
    }
    #[cold]
    #[inline(never)]
    fn pop_u64_slow(&mut self) -> u64 {
        self.sub_top(8);
        trusted_read_u64(&self.memory[self.top..self.top + 8])
    }
    #[inline(always)]
    pub fn pop_u64(&mut self) -> u64 {
        if self.cache.width == CachedOperandWidth::Eight {
            let bits = self.cache.bits;
            self.top -= 8;
            self.cache.clear();
            return bits;
        }
        self.pop_u64_slow()
    }
    #[inline(always)]
    pub fn pop_u32_bytes(&mut self) -> [u8; 4] {
        self.pop_u32().to_le_bytes()
    }
    #[inline(always)]
    pub fn pop_u64_bytes(&mut self) -> [u8; 8] {
        self.pop_u64().to_le_bytes()
    }
    pub fn push_i32(&mut self, v: i32) -> VMResult<()> {
        self.push_u32(v as u32)
    }
    pub fn push_f32(&mut self, v: f32) -> VMResult<()> {
        self.push_u32(v.to_bits())
    }
    pub fn push_f64(&mut self, v: f64) -> VMResult<()> {
        self.push_u64(v.to_bits())
    }
    pub fn pop_i32(&mut self) -> i32 {
        self.pop_u32() as i32
    }
    pub fn push_i64(&mut self, v: i64) -> VMResult<()> {
        self.push_u64(v as u64)
    }
    pub fn pop_i64(&mut self) -> i64 {
        self.pop_u64() as i64
    }
    pub fn pop_f32(&mut self) -> f32 {
        f32::from_bits(self.pop_u32())
    }
    pub fn pop_f64(&mut self) -> f64 {
        f64::from_bits(self.pop_u64())
    }
    #[inline(always)]
    pub fn peek_top_u32(&self) -> u32 {
        self.peek_cached_operand(CachedOperandWidth::Four)
            .map(|bits| bits as u32)
            .unwrap_or_else(|| trusted_read_u32(&self.memory[self.top - 4..self.top]))
    }
    #[inline(always)]
    pub fn peek_top_u64(&self) -> u64 {
        self.peek_cached_operand(CachedOperandWidth::Eight)
            .unwrap_or_else(|| trusted_read_u64(&self.memory[self.top - 8..self.top]))
    }
    #[inline(always)]
    pub fn replace_top_u32(&mut self, value: u32) {
        debug_assert!(self.top >= 4);
        debug_assert!(matches!(
            self.cache.width,
            CachedOperandWidth::None | CachedOperandWidth::Four
        ));
        if self.cache.width == CachedOperandWidth::Four {
            self.cache.bits = u64::from(value);
        } else {
            self.set_cached_operand(CachedOperandWidth::Four, u64::from(value));
        }
    }
    #[inline(always)]
    pub fn replace_top_u64(&mut self, value: u64) {
        debug_assert!(self.top >= 8);
        debug_assert!(matches!(
            self.cache.width,
            CachedOperandWidth::None | CachedOperandWidth::Eight
        ));
        if self.cache.width == CachedOperandWidth::Eight {
            self.cache.bits = value;
        } else {
            self.set_cached_operand(CachedOperandWidth::Eight, value);
        }
    }
    #[inline(always)]
    pub fn narrow_top_u64_to_u32(&mut self, value: u32) {
        debug_assert!(self.top >= 8);
        debug_assert!(matches!(
            self.cache.width,
            CachedOperandWidth::None | CachedOperandWidth::Eight
        ));
        self.top -= 4;
        self.set_cached_operand(CachedOperandWidth::Four, u64::from(value));
    }
    #[inline(always)]
    pub fn pop2_u32(&mut self) -> (u32, u32) {
        match self.cache.width {
            CachedOperandWidth::Four => {
                let rhs = self.cache.bits as u32;
                self.cache.clear();
                self.top -= 4;
                let lhs = trusted_read_u32(&self.memory[self.top - 4..self.top]);
                (lhs, rhs)
            }
            CachedOperandWidth::None => {
                let rhs = trusted_read_u32(&self.memory[self.top - 4..self.top]);
                let lhs = trusted_read_u32(&self.memory[self.top - 8..self.top - 4]);
                self.top -= 4;
                (lhs, rhs)
            }
            CachedOperandWidth::Eight => unreachable!("validated i32 pair expected"),
        }
    }
    #[inline(always)]
    pub fn pop2_u64(&mut self) -> (u64, u64) {
        match self.cache.width {
            CachedOperandWidth::Eight => {
                let rhs = self.cache.bits;
                self.cache.clear();
                self.top -= 8;
                let lhs = trusted_read_u64(&self.memory[self.top - 8..self.top]);
                (lhs, rhs)
            }
            CachedOperandWidth::None => {
                let rhs = trusted_read_u64(&self.memory[self.top - 8..self.top]);
                let lhs = trusted_read_u64(&self.memory[self.top - 16..self.top - 8]);
                self.top -= 8;
                (lhs, rhs)
            }
            CachedOperandWidth::Four => unreachable!("validated i64 pair expected"),
        }
    }
    #[inline(always)]
    pub fn select_top_u32(&mut self, cond: u32) {
        let (lhs, rhs) = self.pop2_u32();
        self.replace_top_u32(if cond == 0 { rhs } else { lhs });
    }
    #[inline(always)]
    pub fn select_top_u64(&mut self, cond: u32) {
        let (lhs, rhs) = self.pop2_u64();
        self.replace_top_u64(if cond == 0 { rhs } else { lhs });
    }
    #[inline(always)]
    fn move_top_scalar4_to(&mut self, dst: usize) {
        let src = self.top - 4;
        if src == dst {
            self.top = dst + 4;
            return;
        }
        let value = match self.cache.width {
            CachedOperandWidth::Four => self.cache.bits as u32,
            CachedOperandWidth::None => trusted_read_u32(&self.memory[src..src + 4]),
            CachedOperandWidth::Eight => unreachable!("validated 4-byte block return"),
        };
        self.top = dst + 4;
        self.set_cached_operand(CachedOperandWidth::Four, u64::from(value));
    }
    #[inline(always)]
    fn move_top_scalar8_to(&mut self, dst: usize) {
        let src = self.top - 8;
        if src == dst {
            self.top = dst + 8;
            return;
        }
        let value = match self.cache.width {
            CachedOperandWidth::Eight => self.cache.bits,
            CachedOperandWidth::None => trusted_read_u64(&self.memory[src..src + 8]),
            CachedOperandWidth::Four => unreachable!("validated 8-byte block return"),
        };
        self.top = dst + 8;
        self.set_cached_operand(CachedOperandWidth::Eight, value);
    }
    #[cold]
    #[inline(never)]
    fn move_top_generic_to(&mut self, dst: usize, return_size: usize) {
        let src = self.top - return_size;
        if src == dst {
            self.cache = OperandCache::EMPTY;
            self.top = dst + return_size;
            return;
        }
        self.flush_cached_operands();
        self.memory.copy_within(src..self.top, dst);
        self.top = dst + return_size;
    }
    #[inline(always)]
    fn move_top_value_to_dst(&mut self, dst: usize, size: usize, shape: ReturnShape) {
        match shape {
            ReturnShape::Empty => {
                self.cache = OperandCache::EMPTY;
                self.top = dst;
            }
            ReturnShape::Scalar4 => self.move_top_scalar4_to(dst),
            ReturnShape::Scalar8 => self.move_top_scalar8_to(dst),
            ReturnShape::Generic => self.move_top_generic_to(dst, size),
        }
    }
    #[inline(always)]
    fn block_return_dst(reference: &LocalReference, stack_top: usize) -> usize {
        reference.local_top + reference.frame_bytes as usize + stack_top
    }
    #[inline(always)]
    fn precomputed_block_return_dst(
        reference: &LocalReference,
        dst_from_local_top: usize,
    ) -> usize {
        reference.local_top + dst_from_local_top
    }
    pub fn access_locals(&mut self, reference: &LocalReference) -> &mut [u8] {
        self.flush_cached_operands();
        &mut self.memory[reference.local_top..reference.local_top + reference.frame_bytes as usize]
    }
    pub fn local_get(
        &mut self,
        reference: &LocalReference,
        local_addr: usize,
        size: usize,
    ) -> VMResult<()> {
        self.flush_cached_operands();
        let new_top = vm_try!(VMResult::from_option(self.top.checked_add(size), || {
            VMResult::StackOverflow
        }));
        if new_top >= self.memory.len() {
            return VMResult::StackOverflow;
        }
        self.memory.copy_within(
            reference.local_top + local_addr..reference.local_top + local_addr + size,
            self.top,
        );
        self.top = new_top;
        VMResult::Success(())
    }
    #[inline(always)]
    pub fn local_get4(&mut self, reference: &LocalReference, local_addr: usize) -> VMResult<()> {
        self.push_u32(trusted_read_u32(self.local_bytes(reference, local_addr, 4)))
    }
    #[inline(always)]
    pub fn local_get8(&mut self, reference: &LocalReference, local_addr: usize) -> VMResult<()> {
        self.push_u64(trusted_read_u64(self.local_bytes(reference, local_addr, 8)))
    }
    #[inline(always)]
    pub fn local_get16(&mut self, reference: &LocalReference, local_addr: usize) -> VMResult<()> {
        self.push_u128(trusted_read_u128(
            self.local_bytes(reference, local_addr, 16),
        ))
    }
    pub fn local_set(&mut self, reference: &LocalReference, local_addr: usize, size: usize) {
        self.flush_cached_operands();
        self.top -= size;
        self.memory
            .copy_within(self.top..self.top + size, reference.local_top + local_addr);
    }
    #[inline(always)]
    pub fn local_set4(&mut self, reference: &LocalReference, local_addr: usize) {
        let value = self.pop_u32();
        let start = reference.local_top + local_addr;
        trusted_write_u32(&mut self.memory[start..start + 4], value);
    }
    #[inline(always)]
    pub fn local_set8(&mut self, reference: &LocalReference, local_addr: usize) {
        let value = self.pop_u64();
        let start = reference.local_top + local_addr;
        trusted_write_u64(&mut self.memory[start..start + 8], value);
    }
    #[inline(always)]
    pub fn local_set16(&mut self, reference: &LocalReference, local_addr: usize) {
        let value = self.pop_u128();
        let start = reference.local_top + local_addr;
        trusted_write_u128(&mut self.memory[start..start + 16], value);
    }
    pub fn local_bytes(&self, reference: &LocalReference, local_addr: usize, size: usize) -> &[u8] {
        &self.memory[reference.local_top + local_addr..reference.local_top + local_addr + size]
    }
    #[inline(always)]
    pub fn local_read_u32(&self, reference: &LocalReference, local_addr: usize) -> u32 {
        trusted_read_u32(self.local_bytes(reference, local_addr, 4))
    }
    #[inline(always)]
    pub fn local_write_u32(&mut self, reference: &LocalReference, local_addr: usize, value: u32) {
        let start = reference.local_top + local_addr;
        trusted_write_u32(&mut self.memory[start..start + 4], value);
    }
    #[inline(always)]
    pub fn local_read_u64(&self, reference: &LocalReference, local_addr: usize) -> u64 {
        trusted_read_u64(self.local_bytes(reference, local_addr, 8))
    }
    #[inline(always)]
    pub fn local_write_u64(&mut self, reference: &LocalReference, local_addr: usize, value: u64) {
        let start = reference.local_top + local_addr;
        trusted_write_u64(&mut self.memory[start..start + 8], value);
    }
    fn zero_new_locals(&mut self, start: usize, size: usize) -> VMResult<()> {
        if size == 0 {
            return VMResult::Success(());
        }
        let end = vm_try!(VMResult::from_option(start.checked_add(size), || {
            VMResult::StackOverflow
        }));
        vm_try!(VMResult::from_option(
            self.memory.get_mut(start..end),
            || { VMResult::StackOverflow }
        ))
        .fill(0);
        VMResult::Success(())
    }
    /// # Safety
    /// Caller must ensure the returned pointer is not used after the stack is dropped or reallocated.
    pub unsafe fn local_area_mut_ptr(&mut self, reference: &LocalReference) -> *mut u8 {
        self.flush_cached_operands();
        self.memory.as_mut_ptr().add(reference.local_top)
    }
    pub fn local_tee(&mut self, reference: &LocalReference, local_addr: usize, size: usize) {
        self.flush_cached_operands();
        self.memory
            .copy_within(self.top - size..self.top, reference.local_top + local_addr);
    }
    #[inline(always)]
    pub fn local_tee4(&mut self, reference: &LocalReference, local_addr: usize) {
        let value = self.peek_top_u32();
        let start = reference.local_top + local_addr;
        trusted_write_u32(&mut self.memory[start..start + 4], value);
    }
    #[inline(always)]
    pub fn local_tee8(&mut self, reference: &LocalReference, local_addr: usize) {
        let value = self.peek_top_u64();
        let start = reference.local_top + local_addr;
        trusted_write_u64(&mut self.memory[start..start + 8], value);
    }
    #[inline(always)]
    pub fn local_tee16(&mut self, reference: &LocalReference, local_addr: usize) {
        let value = trusted_read_u128(&self.memory[self.top - 16..self.top]);
        let start = reference.local_top + local_addr;
        trusted_write_u128(&mut self.memory[start..start + 16], value);
    }
    pub fn function_call_raw_with_return_pc<F: IntoCallFrameCache>(
        &mut self,
        param_size: usize,
        local_size: usize,
        frame: F,
        prev_local_reference: LocalReference,
        return_pc: StablePc,
        runtime: &StoreInner,
    ) -> VMResult<LocalReference> {
        self.flush_cached_operands();
        let frame = frame.into_call_frame_cache(runtime);
        let local_top = vm_try!(VMResult::from_option(
            self.top.checked_sub(param_size),
            || VMResult::StackOverflow
        ));

        vm_try!(self.add_top(local_size));
        vm_try!(self.zero_new_locals(local_top + param_size, local_size));
        let info = CallStackInfo {
            return_pc,
            code_addr: frame.code_addr,
            code_base: frame.code_base,
            code_len: frame.code_len,
            function_return_site_addr: frame.function_return_site_addr,
            instance: frame.instance,
            memory0_kind: frame.memory0_kind,
            memory0_raw: frame.memory0_raw,
            prev_local_reference_top: prev_local_reference.local_top,
            prev_local_reference_frame_bytes: prev_local_reference.frame_bytes,
            prev_local_reference_layout: prev_local_reference.layout,
        };
        vm_try!(self.push_call_stack_info(info));

        VMResult::Success(LocalReference::from_raw(
            local_top,
            (param_size + local_size + std::mem::size_of::<CallStackInfo>()) as u32,
        ))
    }
    pub fn function_call_raw<F: IntoCallFrameCache>(
        &mut self,
        param_size: usize,
        local_size: usize,
        frame: F,
        prev_local_reference: LocalReference,
        return_addr: *const Instr,
        runtime: &StoreInner,
    ) -> VMResult<LocalReference> {
        self.function_call_raw_with_return_pc(
            param_size,
            local_size,
            frame,
            prev_local_reference,
            StablePc::from_raw_in_frame(runtime, self, prev_local_reference, return_addr),
            runtime,
        )
    }
    pub fn function_call_layout_with_return_pc<F: IntoCallFrameCache>(
        &mut self,
        layout: &FrameLayoutHeader,
        frame: F,
        prev_local_reference: LocalReference,
        return_pc: StablePc,
        runtime: &StoreInner,
    ) -> VMResult<LocalReference> {
        self.flush_cached_operands();
        let frame = frame.into_call_frame_cache(runtime);
        let local_top = vm_try!(VMResult::from_option(
            self.top.checked_sub(layout.param_bytes as usize),
            || VMResult::StackOverflow
        ));

        vm_try!(self.add_top(layout.locals_bytes as usize));
        vm_try!(self.zero_new_locals(
            local_top + layout.locals_zero_start_from_local_top as usize,
            layout.locals_bytes as usize,
        ));
        let info = CallStackInfo {
            return_pc,
            code_addr: frame.code_addr,
            code_base: frame.code_base,
            code_len: frame.code_len,
            function_return_site_addr: frame.function_return_site_addr,
            instance: frame.instance,
            memory0_kind: frame.memory0_kind,
            memory0_raw: frame.memory0_raw,
            prev_local_reference_top: prev_local_reference.local_top,
            prev_local_reference_frame_bytes: prev_local_reference.frame_bytes,
            prev_local_reference_layout: prev_local_reference.layout,
        };
        vm_try!(self.push_call_stack_info(info));

        VMResult::Success(LocalReference::from_layout(local_top, layout))
    }
    pub fn function_call<F: IntoCallFrameCache>(
        &mut self,
        param_size: usize,
        local_size: usize,
        frame: F,
        prev_local_reference: LocalReference,
        return_addr: *const Instr,
        runtime: &StoreInner,
    ) -> VMResult<LocalReference> {
        self.function_call_raw(
            param_size,
            local_size,
            frame,
            prev_local_reference,
            return_addr,
            runtime,
        )
    }
    pub fn function_call_layout<F: IntoCallFrameCache>(
        &mut self,
        layout: &FrameLayoutHeader,
        frame: F,
        prev_local_reference: LocalReference,
        return_addr: *const Instr,
        runtime: &StoreInner,
    ) -> VMResult<LocalReference> {
        self.function_call_layout_with_return_pc(
            layout,
            frame,
            prev_local_reference,
            StablePc::from_raw_in_frame(runtime, self, prev_local_reference, return_addr),
            runtime,
        )
    }
    pub(crate) fn function_return_with_frame(
        &mut self,
        reference: &LocalReference,
        return_size: usize,
        runtime: &StoreInner,
    ) -> (LocalReference, CallFrameCache, *const Instr) {
        self.flush_cached_operands();
        let prev_local_reference = self.previous_local_reference(reference);
        let return_pc = self.return_pc(reference);

        self.memory
            .copy_within(self.top - return_size..self.top, reference.local_top);
        self.top = reference.local_top + return_size;
        let prev_frame = if prev_local_reference.has_call_stack_info() {
            self.frame_cache(&prev_local_reference)
        } else {
            CallFrameCache::dummy()
        };
        (
            prev_local_reference,
            prev_frame,
            return_pc.resolve(runtime, self, prev_local_reference),
        )
    }

    pub fn function_return(
        &mut self,
        reference: &LocalReference,
        return_size: usize,
        runtime: &StoreInner,
    ) -> (LocalReference, *const Instr) {
        let (prev_local_reference, _prev_frame, tail_code) =
            self.function_return_with_frame(reference, return_size, runtime);
        (prev_local_reference, tail_code)
    }

    pub(crate) fn function_return_empty_with_frame(
        &mut self,
        reference: &LocalReference,
        runtime: &StoreInner,
    ) -> (LocalReference, CallFrameCache, *const Instr) {
        self.flush_cached_operands();
        let prev_local_reference = self.previous_local_reference(reference);
        let return_pc = self.return_pc(reference);
        self.cache = OperandCache::EMPTY;
        self.top = reference.local_top;
        let prev_frame = if prev_local_reference.has_call_stack_info() {
            self.frame_cache(&prev_local_reference)
        } else {
            CallFrameCache::dummy()
        };
        (
            prev_local_reference,
            prev_frame,
            return_pc.resolve(runtime, self, prev_local_reference),
        )
    }

    pub fn function_return_empty(
        &mut self,
        reference: &LocalReference,
        runtime: &StoreInner,
    ) -> (LocalReference, *const Instr) {
        let (prev_local_reference, _prev_frame, tail_code) =
            self.function_return_empty_with_frame(reference, runtime);
        (prev_local_reference, tail_code)
    }

    pub(crate) fn function_return4_with_frame(
        &mut self,
        reference: &LocalReference,
        runtime: &StoreInner,
    ) -> (LocalReference, CallFrameCache, *const Instr) {
        let prev_local_reference = self.previous_local_reference(reference);
        let return_pc = self.return_pc(reference);
        let value = match self.cache.width {
            CachedOperandWidth::Four => self.cache.bits as u32,
            CachedOperandWidth::None => trusted_read_u32(&self.memory[self.top - 4..self.top]),
            CachedOperandWidth::Eight => unreachable!("validated 4-byte function return"),
        };
        self.top = reference.local_top + 4;
        self.set_cached_operand(CachedOperandWidth::Four, u64::from(value));
        let prev_frame = if prev_local_reference.has_call_stack_info() {
            self.frame_cache(&prev_local_reference)
        } else {
            CallFrameCache::dummy()
        };
        (
            prev_local_reference,
            prev_frame,
            return_pc.resolve(runtime, self, prev_local_reference),
        )
    }

    pub fn function_return4(
        &mut self,
        reference: &LocalReference,
        runtime: &StoreInner,
    ) -> (LocalReference, *const Instr) {
        let (prev_local_reference, _prev_frame, tail_code) =
            self.function_return4_with_frame(reference, runtime);
        (prev_local_reference, tail_code)
    }

    pub(crate) fn function_return8_with_frame(
        &mut self,
        reference: &LocalReference,
        runtime: &StoreInner,
    ) -> (LocalReference, CallFrameCache, *const Instr) {
        let prev_local_reference = self.previous_local_reference(reference);
        let return_pc = self.return_pc(reference);
        let value = match self.cache.width {
            CachedOperandWidth::Eight => self.cache.bits,
            CachedOperandWidth::None => trusted_read_u64(&self.memory[self.top - 8..self.top]),
            CachedOperandWidth::Four => unreachable!("validated 8-byte function return"),
        };
        self.top = reference.local_top + 8;
        self.set_cached_operand(CachedOperandWidth::Eight, value);
        let prev_frame = if prev_local_reference.has_call_stack_info() {
            self.frame_cache(&prev_local_reference)
        } else {
            CallFrameCache::dummy()
        };
        (
            prev_local_reference,
            prev_frame,
            return_pc.resolve(runtime, self, prev_local_reference),
        )
    }

    pub fn function_return8(
        &mut self,
        reference: &LocalReference,
        runtime: &StoreInner,
    ) -> (LocalReference, *const Instr) {
        let (prev_local_reference, _prev_frame, tail_code) =
            self.function_return8_with_frame(reference, runtime);
        (prev_local_reference, tail_code)
    }

    pub(crate) fn function_return_shaped_with_frame(
        &mut self,
        reference: &LocalReference,
        return_size: usize,
        shape: ReturnShape,
        runtime: &StoreInner,
    ) -> (LocalReference, CallFrameCache, *const Instr) {
        match shape {
            ReturnShape::Empty => self.function_return_empty_with_frame(reference, runtime),
            ReturnShape::Scalar4 => self.function_return4_with_frame(reference, runtime),
            ReturnShape::Scalar8 => self.function_return8_with_frame(reference, runtime),
            ReturnShape::Generic => {
                self.function_return_with_frame(reference, return_size, runtime)
            }
        }
    }

    pub fn function_return_shaped(
        &mut self,
        reference: &LocalReference,
        return_size: usize,
        shape: ReturnShape,
        runtime: &StoreInner,
    ) -> (LocalReference, *const Instr) {
        let (prev_local_reference, _prev_frame, tail_code) =
            self.function_return_shaped_with_frame(reference, return_size, shape, runtime);
        (prev_local_reference, tail_code)
    }
    /// Like `function_return` but assumes the return data is already written at `local_top`.
    pub(crate) fn function_return_in_place_with_frame(
        &mut self,
        reference: &LocalReference,
        return_size: usize,
        runtime: &StoreInner,
    ) -> (LocalReference, CallFrameCache, *const Instr) {
        self.function_return_in_place_shaped_with_frame(
            reference,
            return_size,
            ReturnShape::from_size(return_size as u32),
            runtime,
        )
    }

    pub fn function_return_in_place(
        &mut self,
        reference: &LocalReference,
        return_size: usize,
        runtime: &StoreInner,
    ) -> (LocalReference, *const Instr) {
        let (prev_local_reference, _prev_frame, tail_code) =
            self.function_return_in_place_with_frame(reference, return_size, runtime);
        (prev_local_reference, tail_code)
    }

    pub(crate) fn function_return_in_place_shaped_with_frame(
        &mut self,
        reference: &LocalReference,
        return_size: usize,
        _shape: ReturnShape,
        runtime: &StoreInner,
    ) -> (LocalReference, CallFrameCache, *const Instr) {
        self.flush_cached_operands();
        let prev_local_reference = self.previous_local_reference(reference);
        let return_pc = self.return_pc(reference);
        self.cache = OperandCache::EMPTY;
        self.top = reference.local_top + return_size;
        let prev_frame = if prev_local_reference.has_call_stack_info() {
            self.frame_cache(&prev_local_reference)
        } else {
            CallFrameCache::dummy()
        };
        (
            prev_local_reference,
            prev_frame,
            return_pc.resolve(runtime, self, prev_local_reference),
        )
    }

    pub fn function_return_in_place_shaped(
        &mut self,
        reference: &LocalReference,
        return_size: usize,
        shape: ReturnShape,
        runtime: &StoreInner,
    ) -> (LocalReference, *const Instr) {
        let (prev_local_reference, _prev_frame, tail_code) =
            self.function_return_in_place_shaped_with_frame(reference, return_size, shape, runtime);
        (prev_local_reference, tail_code)
    }
    pub fn function_return_call(
        &mut self,
        reference: &LocalReference,
        param_size: usize,
        param_shape: ReturnShape,
        local_size: usize,
        frame: CallFrameCache,
    ) -> VMResult<LocalReference> {
        self.function_return_call_raw(reference, param_size, param_shape, local_size, frame)
    }
    pub fn function_return_call_layout(
        &mut self,
        reference: &LocalReference,
        layout: &FrameLayoutHeader,
        frame: CallFrameCache,
    ) -> VMResult<LocalReference> {
        self.flush_cached_operands();
        tracing::trace!("function_return_call: {reference:?}");
        let return_pc = self.return_pc(reference);
        let prev_local_reference = self.previous_local_reference_footer(reference);
        self.move_top_value_to_dst(
            reference.local_top,
            layout.param_bytes as usize,
            layout.param_shape,
        );
        vm_try!(self.add_top(layout.locals_bytes as usize));
        vm_try!(self.zero_new_locals(
            reference.local_top + layout.locals_zero_start_from_local_top as usize,
            layout.locals_bytes as usize,
        ));

        let info = CallStackInfo {
            return_pc,
            code_addr: frame.code_addr,
            code_base: frame.code_base,
            code_len: frame.code_len,
            function_return_site_addr: frame.function_return_site_addr,
            instance: frame.instance,
            memory0_kind: frame.memory0_kind,
            memory0_raw: frame.memory0_raw,
            prev_local_reference_top: prev_local_reference.local_top,
            prev_local_reference_frame_bytes: prev_local_reference.frame_bytes,
            prev_local_reference_layout: prev_local_reference.layout,
        };
        vm_try!(self.push_call_stack_info(info));

        VMResult::Success(LocalReference::from_layout(reference.local_top, layout))
    }
    pub fn function_return_call_raw(
        &mut self,
        reference: &LocalReference,
        param_size: usize,
        param_shape: ReturnShape,
        local_size: usize,
        frame: CallFrameCache,
    ) -> VMResult<LocalReference> {
        self.flush_cached_operands();
        tracing::trace!("function_return_call: {reference:?}");
        let return_pc = self.return_pc(reference);
        let prev_local_reference = self.previous_local_reference_footer(reference);
        self.move_top_value_to_dst(reference.local_top, param_size, param_shape);
        vm_try!(self.add_top(local_size));
        vm_try!(self.zero_new_locals(reference.local_top + param_size, local_size));

        let info = CallStackInfo {
            return_pc,
            code_addr: frame.code_addr,
            code_base: frame.code_base,
            code_len: frame.code_len,
            function_return_site_addr: frame.function_return_site_addr,
            instance: frame.instance,
            memory0_kind: frame.memory0_kind,
            memory0_raw: frame.memory0_raw,
            prev_local_reference_top: prev_local_reference.local_top,
            prev_local_reference_frame_bytes: prev_local_reference.frame_bytes,
            prev_local_reference_layout: prev_local_reference.layout,
        };
        vm_try!(self.push_call_stack_info(info));

        VMResult::Success(LocalReference::from_raw(
            reference.local_top,
            (param_size + local_size + std::mem::size_of::<CallStackInfo>()) as u32,
        ))
    }
    pub fn block_return(
        &mut self,
        reference: &LocalReference,
        stack_top: usize,
        return_size: usize,
    ) {
        match ReturnShape::from_size(return_size as u32) {
            ReturnShape::Empty => self.block_return_empty(reference, stack_top),
            ReturnShape::Scalar4 => self.block_return4(reference, stack_top),
            ReturnShape::Scalar8 => self.block_return8(reference, stack_top),
            ReturnShape::Generic => self.block_return_generic(reference, stack_top, return_size),
        }
    }

    pub fn block_return_empty(&mut self, reference: &LocalReference, stack_top: usize) {
        self.cache = OperandCache::EMPTY;
        self.top = Self::block_return_dst(reference, stack_top);
    }

    pub fn block_return4(&mut self, reference: &LocalReference, stack_top: usize) {
        self.move_top_scalar4_to(Self::block_return_dst(reference, stack_top));
    }

    pub fn block_return8(&mut self, reference: &LocalReference, stack_top: usize) {
        self.move_top_scalar8_to(Self::block_return_dst(reference, stack_top));
    }

    pub fn block_return_generic(
        &mut self,
        reference: &LocalReference,
        stack_top: usize,
        return_size: usize,
    ) {
        self.move_top_generic_to(Self::block_return_dst(reference, stack_top), return_size);
    }

    pub fn block_return_shaped(
        &mut self,
        reference: &LocalReference,
        stack_top: usize,
        return_size: usize,
        shape: ReturnShape,
    ) {
        match shape {
            ReturnShape::Empty => self.block_return_empty(reference, stack_top),
            ReturnShape::Scalar4 => self.block_return4(reference, stack_top),
            ReturnShape::Scalar8 => self.block_return8(reference, stack_top),
            ReturnShape::Generic => self.block_return_generic(reference, stack_top, return_size),
        }
    }

    pub fn block_return_empty_precomputed(
        &mut self,
        reference: &LocalReference,
        dst_from_local_top: usize,
    ) {
        self.cache = OperandCache::EMPTY;
        self.top = Self::precomputed_block_return_dst(reference, dst_from_local_top);
    }

    pub fn block_return4_precomputed(
        &mut self,
        reference: &LocalReference,
        dst_from_local_top: usize,
    ) {
        self.move_top_scalar4_to(Self::precomputed_block_return_dst(
            reference,
            dst_from_local_top,
        ));
    }

    pub fn block_return8_precomputed(
        &mut self,
        reference: &LocalReference,
        dst_from_local_top: usize,
    ) {
        self.move_top_scalar8_to(Self::precomputed_block_return_dst(
            reference,
            dst_from_local_top,
        ));
    }

    pub fn block_return_generic_precomputed(
        &mut self,
        reference: &LocalReference,
        dst_from_local_top: usize,
        return_size: usize,
    ) {
        self.move_top_generic_to(
            Self::precomputed_block_return_dst(reference, dst_from_local_top),
            return_size,
        );
    }
}
pub(crate) trait StackOperation<T> {
    fn push(&mut self, v: T) -> VMResult<()>;
    fn pop(&mut self) -> T;
}

macro_rules! stack_operation {
    ($target: ident,$push_op: ident,$pop_op: ident) => {
        impl StackOperation<$target> for Stack {
            fn push(&mut self, v: $target) -> VMResult<()> {
                self.$push_op(v)
            }

            fn pop(&mut self) -> $target {
                self.$pop_op()
            }
        }
    };
}
stack_operation!(u32, push_u32, pop_u32);
stack_operation!(u64, push_u64, pop_u64);
stack_operation!(i32, push_i32, pop_i32);
stack_operation!(i64, push_i64, pop_i64);
stack_operation!(f32, push_f32, pop_f32);
stack_operation!(f64, push_f64, pop_f64);
macro_rules! stack_operation_wide {
    ($target: ty) => {
        #[cfg(feature = "simd")]
        impl StackOperation<$target> for Stack {
            fn pop(&mut self) -> $target {
                let x = self.pop_u8_array::<16>();
                From::<[<$target as LaneType>::BaseType; <$target as LaneType>::LANE_SIZE]>::from(
                    unsafe {
                        #[allow(clippy::useless_transmute)]
                        std::mem::transmute::<
                            [u8; 16],
                            [<$target as LaneType>::BaseType; <$target as LaneType>::LANE_SIZE],
                        >(x)
                    },
                )
            }
            fn push(&mut self, v: $target) -> VMResult<()> {
                let x: [u8; 16] = unsafe {
                    #[allow(clippy::useless_transmute)]
                    std::mem::transmute::<
                        [<$target as LaneType>::BaseType; <$target as LaneType>::LANE_SIZE],
                        [u8; 16],
                    >(v.to_array())
                };
                self.push_u8_array(x)
            }
        }
    };
}
#[cfg(feature = "simd")]
stack_operation_wide!(f32x4);
#[cfg(feature = "simd")]
stack_operation_wide!(f64x2);
#[cfg(feature = "simd")]
stack_operation_wide!(i8x16);
#[cfg(feature = "simd")]
stack_operation_wide!(i16x8);
#[cfg(feature = "simd")]
stack_operation_wide!(i32x4);
#[cfg(feature = "simd")]
stack_operation_wide!(i64x2);
#[cfg(feature = "simd")]
stack_operation_wide!(u8x16);
#[cfg(feature = "simd")]
stack_operation_wide!(u16x8);
#[cfg(feature = "simd")]
stack_operation_wide!(u32x4);
#[cfg(feature = "simd")]
stack_operation_wide!(u64x2);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{FrameLayoutMetadata, Operand, SafepointMetadataCache};
    use std::sync::Arc;

    fn frame(kind: CachedMemoryKind, raw: u32) -> CallFrameCache {
        CallFrameCache {
            code_addr: ObjectRef(0),
            code_base: std::ptr::null(),
            code_len: 0,
            function_return_site_addr: 0,
            instance: InstanceId::from_index(0),
            memory0_kind: kind,
            memory0_raw: raw,
        }
    }

    fn frame_with_code(kind: CachedMemoryKind, raw: u32, code: &[Instr]) -> CallFrameCache {
        CallFrameCache {
            code_addr: ObjectRef(1),
            code_base: code.as_ptr(),
            code_len: u32::try_from(code.len()).unwrap(),
            function_return_site_addr: 0,
            instance: InstanceId::from_index(0),
            memory0_kind: kind,
            memory0_raw: raw,
        }
    }

    fn layout(
        param_bytes: u32,
        locals_bytes: u32,
        param_shape: ReturnShape,
        result_shape: ReturnShape,
    ) -> FrameLayoutMetadata {
        FrameLayoutMetadata::new(
            param_bytes,
            locals_bytes,
            0,
            param_shape,
            result_shape,
            crate::common::FrameLayoutColdMetadata {
                local_slots: Arc::from([]),
                local_ref_runs: Arc::from([]),
                stack_map_sites: Arc::from([]),
                unwind_sites: Arc::from([]),
                instruction_ordinal_by_raw_start: Arc::from([]),
            },
        )
    }

    fn layout_with_cold(
        param_bytes: u32,
        locals_bytes: u32,
        param_shape: ReturnShape,
        result_shape: ReturnShape,
        cold: crate::common::FrameLayoutColdMetadata,
    ) -> FrameLayoutMetadata {
        FrameLayoutMetadata::new(
            param_bytes,
            locals_bytes,
            0,
            param_shape,
            result_shape,
            cold,
        )
    }

    #[test]
    fn local_reference_tracks_layout_for_wasm_and_raw_frames() {
        let frame_layout = layout(4, 8, ReturnShape::Scalar4, ReturnShape::Scalar4);
        let raw = LocalReference::from_raw(12, 24);
        let wasm = LocalReference::from_layout(16, &frame_layout);
        let raw_layout = raw.layout;
        let raw_frame_bytes = raw.frame_bytes;
        let wasm_layout = wasm.layout;
        let wasm_frame_bytes = wasm.frame_bytes;

        assert!(raw_layout.is_none());
        assert_eq!(raw_frame_bytes, 24);
        assert!(wasm_layout.is_some());
        assert_eq!(wasm_frame_bytes, frame_layout.fixed_frame_bytes);
    }

    #[test]
    fn local_and_operand_ref_ranges_follow_precomputed_layout_metadata() {
        let stack = Stack::new(128);
        let frame_layout = layout_with_cold(
            4,
            8,
            ReturnShape::Scalar4,
            ReturnShape::Scalar4,
            crate::common::FrameLayoutColdMetadata {
                local_slots: Arc::from([]),
                local_ref_runs: Arc::from([
                    crate::common::RefSlotRun {
                        start_from_local_top: 0,
                        len_bytes: 4,
                    },
                    crate::common::RefSlotRun {
                        start_from_local_top: 12,
                        len_bytes: 4,
                    },
                ]),
                stack_map_sites: Arc::from([crate::common::StackMapSite {
                    instruction_ordinal: 7,
                    kind: crate::common::StackMapSafepointKind::Call,
                    operand_bytes: 8,
                    ref_offsets_from_operand_base: Arc::from([0, 4]),
                }]),
                unwind_sites: Arc::from([crate::common::UnwindSiteMetadata {
                    instruction_ordinal: 7,
                    kind: crate::common::StackMapSafepointKind::Call,
                    result_slot_from_local_top: Some(20),
                }]),
                instruction_ordinal_by_raw_start: Arc::from([]),
            },
        );
        let reference = LocalReference::from_layout(32, &frame_layout);

        let mut local_ranges = Vec::new();
        stack.visit_local_ref_ranges(&reference, |range| local_ranges.push(range));
        assert_eq!(local_ranges, vec![32..36, 44..48]);

        let stack_map_site = frame_layout.stack_map_site(7).unwrap();
        let operand_base = reference.local_top + frame_layout.operand_base_from_local_top as usize;
        let mut operand_ranges = Vec::new();
        stack.visit_operand_ref_ranges(&reference, stack_map_site, |range| {
            operand_ranges.push(range)
        });
        assert_eq!(
            operand_ranges,
            vec![
                operand_base..operand_base + 4,
                operand_base + 4..operand_base + 8
            ]
        );

        let unwind_site = frame_layout.unwind_site(7).unwrap();
        assert_eq!(unwind_site.result_slot_from_local_top, Some(20));
    }

    #[test]
    fn safepoint_cache_drives_pointer_based_ref_and_unwind_helpers() {
        let stack = Stack::new(128);
        let frame_layout = layout_with_cold(
            4,
            8,
            ReturnShape::Scalar4,
            ReturnShape::Scalar4,
            crate::common::FrameLayoutColdMetadata {
                local_slots: Arc::from([]),
                local_ref_runs: Arc::from([crate::common::RefSlotRun {
                    start_from_local_top: 0,
                    len_bytes: 4,
                }]),
                stack_map_sites: Arc::from([crate::common::StackMapSite {
                    instruction_ordinal: 7,
                    kind: crate::common::StackMapSafepointKind::Call,
                    operand_bytes: 8,
                    ref_offsets_from_operand_base: Arc::from([0]),
                }]),
                unwind_sites: Arc::from([crate::common::UnwindSiteMetadata {
                    instruction_ordinal: 7,
                    kind: crate::common::StackMapSafepointKind::Call,
                    result_slot_from_local_top: Some(16),
                }]),
                instruction_ordinal_by_raw_start: Arc::from([]),
            },
        );
        let reference = LocalReference::from_layout(32, &frame_layout);
        let stack_map_site = frame_layout.stack_map_site(7).unwrap();
        let unwind_site = frame_layout.unwind_site(7).unwrap();
        let safepoint = SafepointMetadataCache::new(
            stack_map_site as *const _ as usize,
            unwind_site as *const _ as usize,
        );

        let mut ranges = Vec::new();
        stack.visit_local_and_operand_ref_ranges(&reference, safepoint, |range| ranges.push(range));

        let operand_base = reference.local_top + frame_layout.operand_base_from_local_top as usize;
        assert_eq!(ranges, vec![32..36, operand_base..operand_base + 4]);
        assert_eq!(
            stack.result_slot_from_unwind_site(&reference, safepoint.unwind_site_ptr()),
            Some(16)
        );
    }

    #[test]
    fn control_unwind_sites_translate_operand_offsets_to_local_offsets() {
        let stack = Stack::new(128);
        let frame_layout = layout_with_cold(
            4,
            8,
            ReturnShape::Scalar4,
            ReturnShape::Scalar4,
            crate::common::FrameLayoutColdMetadata {
                local_slots: Arc::from([]),
                local_ref_runs: Arc::from([]),
                stack_map_sites: Arc::from([]),
                unwind_sites: Arc::from([crate::common::UnwindSiteMetadata {
                    instruction_ordinal: 9,
                    kind: crate::common::StackMapSafepointKind::Loop,
                    result_slot_from_local_top: Some(8),
                }]),
                instruction_ordinal_by_raw_start: Arc::from([]),
            },
        );
        let reference = LocalReference::from_layout(32, &frame_layout);
        let unwind_site = frame_layout.unwind_site(9).unwrap();

        assert_eq!(
            stack.result_slot_from_unwind_site(&reference, Some(unwind_site as *const _)),
            Some(frame_layout.operand_base_from_local_top as usize + 8)
        );
    }

    #[test]
    fn function_return_with_frame_restores_caller_layout_and_frame_cache() {
        let mut stack = Stack::new(256);
        let runtime = StoreInner::new();
        let caller_layout = layout(0, 4, ReturnShape::Empty, ReturnShape::Empty);
        let callee_layout = layout(0, 4, ReturnShape::Empty, ReturnShape::Scalar4);
        let caller = stack
            .function_call_layout(
                &caller_layout,
                frame(CachedMemoryKind::Local, 1),
                LocalReference::empty(),
                std::ptr::null(),
                &runtime,
            )
            .unwrap();
        let callee = stack
            .function_call_layout(
                &callee_layout,
                frame(CachedMemoryKind::Shared, 2),
                caller,
                std::ptr::null(),
                &runtime,
            )
            .unwrap();

        let previous_local_ref = stack.previous_local_reference(&callee);
        let caller_local_top = caller.local_top;
        let caller_frame_bytes = caller.frame_bytes;
        let caller_layout = caller.layout.map(|layout| layout.as_ptr());
        assert_eq!(
            stack.frame_cache(&callee).memory0_handle(),
            Some(MemoryHandle::Shared(SharedMemoryId::from_raw(2)))
        );
        let previous_local_top = previous_local_ref.local_top;
        let previous_frame_bytes = previous_local_ref.frame_bytes;
        let previous_layout = previous_local_ref.layout.map(|layout| layout.as_ptr());
        assert_eq!(previous_local_top, caller_local_top);
        assert_eq!(previous_frame_bytes, caller_frame_bytes);
        assert_eq!(previous_layout, caller_layout);

        stack.push_u32(0xaabb_ccdd).unwrap();
        let (restored, prev_frame, return_pc) =
            stack.function_return4_with_frame(&callee, &runtime);
        let restored_local_top = restored.local_top;
        let restored_frame_bytes = restored.frame_bytes;
        let restored_layout = restored.layout.map(|layout| layout.as_ptr());
        let caller_local_top = caller.local_top;
        let caller_frame_bytes = caller.frame_bytes;
        let caller_layout_ptr = caller.layout.map(|layout| layout.as_ptr());

        assert!(return_pc.is_null());
        assert_eq!(restored_local_top, caller_local_top);
        assert_eq!(restored_frame_bytes, caller_frame_bytes);
        assert_eq!(restored_layout, caller_layout_ptr);
        assert_eq!(
            prev_frame.memory0_handle(),
            Some(MemoryHandle::Local(LocalMemoryId::from_raw(1)))
        );
    }

    #[test]
    fn function_call_and_return_roundtrip_result_bytes() {
        let mut stack = Stack::new(256);
        let runtime = StoreInner::new();
        let prev = LocalReference::empty();

        stack.push_u32(0x1122_3344).unwrap();
        let reference = stack
            .function_call_raw(
                4,
                8,
                frame(CachedMemoryKind::Local, 9),
                prev,
                std::ptr::null(),
                &runtime,
            )
            .unwrap();

        let reference_local_top = reference.local_top;
        let reference_frame_bytes = reference.frame_bytes;
        assert_eq!(reference_local_top, 0);
        assert_eq!(
            reference_frame_bytes as usize,
            4 + 8 + std::mem::size_of::<CallStackInfo>()
        );
        assert_eq!(trusted_read_u32(&stack.memory[0..4]), 0x1122_3344);
        assert!(stack.memory[4..12].iter().all(|byte| *byte == 0));
        assert_eq!(
            stack.frame_cache(&reference).memory0_handle(),
            Some(MemoryHandle::Local(LocalMemoryId::from_raw(9)))
        );

        stack.push_u32(0xaabb_ccdd).unwrap();
        let (restored, return_pc) = stack.function_return4(&reference, &runtime);
        let restored_local_top = restored.local_top;
        let restored_frame_bytes = restored.frame_bytes;
        assert_eq!(restored_local_top, 0);
        assert_eq!(restored_frame_bytes, 0);
        assert!(return_pc.is_null());
        assert_eq!(stack.top, 4);
        assert_eq!(stack.pop_u32(), 0xaabb_ccdd);
    }

    #[test]
    fn function_call_layout_uses_precomputed_offsets() {
        let mut stack = Stack::new(256);
        let runtime = StoreInner::new();
        let prev = LocalReference::empty();
        let frame_layout = layout(4, 8, ReturnShape::Scalar4, ReturnShape::Scalar4);

        stack.push_u32(0x1122_3344).unwrap();
        let reference = stack
            .function_call_layout(
                &frame_layout,
                frame(CachedMemoryKind::Local, 9),
                prev,
                std::ptr::null(),
                &runtime,
            )
            .unwrap();

        let reference_local_top = reference.local_top;
        let reference_frame_bytes = reference.frame_bytes;
        assert_eq!(reference_local_top, 0);
        assert_eq!(reference_frame_bytes, frame_layout.fixed_frame_bytes);
        assert_eq!(trusted_read_u32(&stack.memory[0..4]), 0x1122_3344);
        assert!(stack.memory[4..12].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn minimal_footer_views_restore_previous_local_reference_and_current_frame() {
        let mut stack = Stack::new(256);
        let runtime = StoreInner::new();
        let caller_layout = layout(0, 4, ReturnShape::Empty, ReturnShape::Empty);
        let callee_layout = layout(4, 8, ReturnShape::Scalar4, ReturnShape::Scalar4);
        let caller = stack
            .function_call_layout(
                &caller_layout,
                frame(CachedMemoryKind::Local, 7),
                LocalReference::empty(),
                std::ptr::null(),
                &runtime,
            )
            .unwrap();

        stack.push_u32(0x1234_5678).unwrap();
        let callee = stack
            .function_call_layout(
                &callee_layout,
                frame(CachedMemoryKind::Shared, 9),
                caller,
                std::ptr::null(),
                &runtime,
            )
            .unwrap();

        let previous = stack.previous_local_reference(&callee);
        let caller_local_top = caller.local_top;
        let caller_frame_bytes = caller.frame_bytes;
        let caller_layout = caller.layout.map(|layout| layout.as_ptr());
        let previous_local_top = previous.local_top;
        let previous_frame_bytes = previous.frame_bytes;
        let previous_layout = previous.layout.map(|layout| layout.as_ptr());
        assert_eq!(previous_local_top, caller_local_top);
        assert_eq!(previous_frame_bytes, caller_frame_bytes);
        assert_eq!(previous_layout, caller_layout);
        assert_eq!(
            stack.frame_cache(&callee).memory0_handle(),
            Some(MemoryHandle::Shared(SharedMemoryId::from_raw(9)))
        );
    }

    #[test]
    fn stable_pc_roundtrips_from_footer_code_base_and_len_without_runtime_lookup() {
        let mut stack = Stack::new(256);
        let runtime = StoreInner::new();
        let caller_layout = layout(0, 4, ReturnShape::Empty, ReturnShape::Empty);
        let callee_layout = layout(4, 8, ReturnShape::Scalar4, ReturnShape::Scalar4);
        let code = [
            Instr {
                operand: Operand { u32: 0 },
            },
            Instr {
                operand: Operand { u32: 1 },
            },
            Instr {
                operand: Operand { u32: 2 },
            },
            Instr {
                operand: Operand { u32: 3 },
            },
        ];

        let caller = stack
            .function_call_layout(
                &caller_layout,
                frame_with_code(CachedMemoryKind::Local, 7, &code),
                LocalReference::empty(),
                std::ptr::null(),
                &runtime,
            )
            .unwrap();

        stack.push_u32(0x1234_5678).unwrap();
        let callee = stack
            .function_call_layout(
                &callee_layout,
                frame(CachedMemoryKind::Local, 9),
                caller,
                unsafe { code.as_ptr().add(2) },
                &runtime,
            )
            .unwrap();

        let return_pc = stack.return_pc(&callee);
        assert_eq!(return_pc.resolve(&runtime, &stack, caller), unsafe {
            code.as_ptr().add(2)
        });
    }

    #[test]
    fn function_return_in_place_uses_local_area_as_result_slot() {
        let mut stack = Stack::new(256);
        let runtime = StoreInner::new();
        let prev = LocalReference::empty();

        let reference = stack
            .function_call_raw(
                0,
                8,
                frame(CachedMemoryKind::Shared, 5),
                prev,
                std::ptr::null(),
                &runtime,
            )
            .unwrap();

        trusted_write_u32(
            &mut stack.memory[reference.local_top..reference.local_top + 4],
            0x5566_7788,
        );
        let (restored, return_pc) = stack.function_return_in_place(&reference, 4, &runtime);
        let restored_local_top = restored.local_top;
        let restored_frame_bytes = restored.frame_bytes;
        assert_eq!(restored_local_top, 0);
        assert_eq!(restored_frame_bytes, 0);
        assert!(return_pc.is_null());
        assert_eq!(stack.top, 4);
        assert_eq!(stack.pop_u32(), 0x5566_7788);
    }

    #[test]
    fn function_return_fast_path_keeps_u32_result_cached() {
        let mut stack = Stack::new(256);
        let runtime = StoreInner::new();
        let prev = LocalReference::empty();

        stack.push_u32(0x0102_0304).unwrap();
        let reference = stack
            .function_call_raw(
                4,
                8,
                frame(CachedMemoryKind::Local, 9),
                prev,
                std::ptr::null(),
                &runtime,
            )
            .unwrap();

        stack.push_u32(0xaabb_ccdd).unwrap();
        let (restored, return_pc) = stack.function_return(&reference, 4, &runtime);
        let restored_local_top = restored.local_top;
        let restored_frame_bytes = restored.frame_bytes;
        assert_eq!(restored_local_top, 0);
        assert_eq!(restored_frame_bytes, 0);
        assert!(return_pc.is_null());
        assert_eq!(stack.pop_u32(), 0xaabb_ccdd);
    }

    #[test]
    fn function_return_call_reuses_frame_slot_and_zeroes_new_locals() {
        let mut stack = Stack::new(256);
        let runtime = StoreInner::new();
        let empty = LocalReference::empty();
        let caller = stack
            .function_call_raw(
                0,
                4,
                frame(CachedMemoryKind::Local, 1),
                empty,
                std::ptr::null(),
                &runtime,
            )
            .unwrap();

        stack.push_u32(0x0102_0304).unwrap();
        let callee = stack
            .function_call_raw(
                4,
                4,
                frame(CachedMemoryKind::Shared, 2),
                caller,
                std::ptr::null(),
                &runtime,
            )
            .unwrap();

        stack.push_u32(0xa1a2_a3a4).unwrap();
        stack.push_u32(0xb1b2_b3b4).unwrap();
        let tail = stack
            .function_return_call_raw(
                &callee,
                8,
                ReturnShape::Generic,
                8,
                frame(CachedMemoryKind::Local, 3),
            )
            .unwrap();

        let tail_local_top = tail.local_top;
        let callee_local_top = callee.local_top;
        assert_eq!(tail_local_top, callee_local_top);
        assert_eq!(
            tail.frame_bytes as usize,
            8 + 8 + std::mem::size_of::<CallStackInfo>()
        );
        assert_eq!(
            trusted_read_u32(&stack.memory[tail.local_top..tail.local_top + 4]),
            0xa1a2_a3a4
        );
        assert_eq!(
            trusted_read_u32(&stack.memory[tail.local_top + 4..tail.local_top + 8]),
            0xb1b2_b3b4
        );
        assert!(stack.memory[tail.local_top + 8..tail.local_top + 16]
            .iter()
            .all(|byte| *byte == 0));
        assert_eq!(
            stack.frame_cache(&tail).memory0_handle(),
            Some(MemoryHandle::Local(LocalMemoryId::from_raw(3)))
        );
    }

    #[test]
    fn function_return_call_layout_reuses_precomputed_frame_bytes() {
        let mut stack = Stack::new(256);
        let runtime = StoreInner::new();
        let empty = LocalReference::empty();
        let caller_layout = layout(0, 4, ReturnShape::Empty, ReturnShape::Empty);
        let callee_layout = layout(4, 4, ReturnShape::Scalar4, ReturnShape::Scalar4);
        let tail_layout = layout(8, 8, ReturnShape::Generic, ReturnShape::Generic);
        let caller = stack
            .function_call_layout(
                &caller_layout,
                frame(CachedMemoryKind::Local, 1),
                empty,
                std::ptr::null(),
                &runtime,
            )
            .unwrap();

        stack.push_u32(0x0102_0304).unwrap();
        let callee = stack
            .function_call_layout(
                &callee_layout,
                frame(CachedMemoryKind::Shared, 2),
                caller,
                std::ptr::null(),
                &runtime,
            )
            .unwrap();

        stack.push_u32(0xa1a2_a3a4).unwrap();
        stack.push_u32(0xb1b2_b3b4).unwrap();
        let tail = stack
            .function_return_call_layout(&callee, &tail_layout, frame(CachedMemoryKind::Local, 3))
            .unwrap();
        let previous_local_ref = stack.previous_local_reference(&tail);
        let caller_local_top = caller.local_top;
        let caller_frame_bytes = caller.frame_bytes;
        let caller_layout = caller.layout.map(|layout| layout.as_ptr());

        let tail_local_top = tail.local_top;
        let tail_frame_bytes = tail.frame_bytes;
        let callee_local_top = callee.local_top;
        assert_eq!(tail_local_top, callee_local_top);
        assert_eq!(tail_frame_bytes, tail_layout.fixed_frame_bytes);
        assert_eq!(
            stack.frame_cache(&tail).memory0_handle(),
            Some(MemoryHandle::Local(LocalMemoryId::from_raw(3)))
        );
        let previous_local_top = previous_local_ref.local_top;
        let previous_frame_bytes = previous_local_ref.frame_bytes;
        let previous_layout = previous_local_ref.layout.map(|layout| layout.as_ptr());
        assert_eq!(previous_local_top, caller_local_top);
        assert_eq!(previous_frame_bytes, caller_frame_bytes);
        assert_eq!(previous_layout, caller_layout);
        assert!(stack.memory[tail_local_top + 8..tail_local_top + 16]
            .iter()
            .all(|byte| *byte == 0));
    }

    #[test]
    fn block_return_moves_result_to_block_stack_top() {
        let mut stack = Stack::new(256);
        let runtime = StoreInner::new();
        let empty = LocalReference::empty();
        let reference = stack
            .function_call_raw(
                0,
                4,
                frame(CachedMemoryKind::Local, 1),
                empty,
                std::ptr::null(),
                &runtime,
            )
            .unwrap();
        let frame_top = reference.local_top + reference.frame_bytes as usize;
        stack.push_u32(0x1111_2222).unwrap();
        stack.push_u32(0x3333_4444).unwrap();
        assert_eq!(frame_top + 8, stack.top);

        stack.block_return4(&reference, 4);
        stack.flush_cached_operands();

        assert_eq!(stack.top, frame_top + 8);
        assert_eq!(
            trusted_read_u32(&stack.memory[frame_top..frame_top + 4]),
            0x1111_2222
        );
        assert_eq!(
            trusted_read_u32(&stack.memory[frame_top + 4..frame_top + 8]),
            0x3333_4444
        );
    }

    #[test]
    fn block_return_fast_path_keeps_u32_result_cached() {
        let mut stack = Stack::new(256);
        let runtime = StoreInner::new();
        let empty = LocalReference::empty();
        let reference = stack
            .function_call_raw(
                0,
                4,
                frame(CachedMemoryKind::Local, 1),
                empty,
                std::ptr::null(),
                &runtime,
            )
            .unwrap();
        let frame_top = reference.local_top + reference.frame_bytes as usize;

        stack.push_u32(0x1111_2222).unwrap();
        stack.push_u32(0x3333_4444).unwrap();
        stack.block_return(&reference, 4, 4);

        assert_eq!(stack.cache.width, CachedOperandWidth::Four);
        assert_eq!(stack.peek_top_u32(), 0x3333_4444);
        assert_eq!(stack.pop_u32(), 0x3333_4444);
        assert_eq!(stack.pop_u32(), 0x1111_2222);
        assert_eq!(stack.top, frame_top);
    }

    #[test]
    fn block_return_fast_path_keeps_u64_result_cached() {
        let mut stack = Stack::new(256);
        let runtime = StoreInner::new();
        let empty = LocalReference::empty();
        let reference = stack
            .function_call_raw(
                0,
                8,
                frame(CachedMemoryKind::Local, 1),
                empty,
                std::ptr::null(),
                &runtime,
            )
            .unwrap();
        let frame_top = reference.local_top + reference.frame_bytes as usize;

        stack.push_u64(0x1111_2222_3333_4444).unwrap();
        stack.push_u64(0x5555_6666_7777_8888).unwrap();
        stack.block_return(&reference, 8, 8);

        assert_eq!(stack.cache.width, CachedOperandWidth::Eight);
        assert_eq!(stack.peek_top_u64(), 0x5555_6666_7777_8888);
        assert_eq!(stack.pop_u64(), 0x5555_6666_7777_8888);
        assert_eq!(stack.pop_u64(), 0x1111_2222_3333_4444);
        assert_eq!(stack.top, frame_top);
    }

    #[test]
    fn block_return_zero_result_discards_cached_scalar() {
        let mut stack = Stack::new(256);
        let runtime = StoreInner::new();
        let empty = LocalReference::empty();
        let reference = stack
            .function_call_raw(
                0,
                4,
                frame(CachedMemoryKind::Local, 1),
                empty,
                std::ptr::null(),
                &runtime,
            )
            .unwrap();
        let frame_top = reference.local_top + reference.frame_bytes as usize;

        stack.push_u32(0xaabb_ccdd).unwrap();
        stack.block_return(&reference, 0, 0);

        assert_eq!(stack.top, frame_top);
        assert_eq!(stack.cache.width, CachedOperandWidth::None);
    }

    #[test]
    fn generic_local_get_copies_bytes_to_operand_stack() {
        let mut stack = Stack::new(64);
        let reference = LocalReference::from_raw(0, 8);

        stack
            .push_slice(&[0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80])
            .unwrap();
        stack.local_get(&reference, 2, 4).unwrap();

        assert_eq!(stack.top, 12);
        assert_eq!(&stack.memory[8..12], &[0x30, 0x40, 0x50, 0x60]);
    }

    #[test]
    fn generic_local_set_moves_top_bytes_into_local_slot() {
        let mut stack = Stack::new(64);
        let reference = LocalReference::from_raw(0, 8);

        stack.push_slice(&[0; 8]).unwrap();
        stack.push_slice(&[0xaa, 0xbb, 0xcc, 0xdd]).unwrap();
        stack.local_set(&reference, 2, 4);

        assert_eq!(stack.top, 8);
        assert_eq!(&stack.memory[2..6], &[0xaa, 0xbb, 0xcc, 0xdd]);
    }

    #[test]
    fn generic_local_tee_keeps_operand_stack_and_updates_local_slot() {
        let mut stack = Stack::new(64);
        let reference = LocalReference::from_raw(0, 8);

        stack.push_slice(&[0; 8]).unwrap();
        stack.push_slice(&[1, 2, 3, 4]).unwrap();
        stack.local_tee(&reference, 1, 4);

        assert_eq!(stack.top, 12);
        assert_eq!(&stack.memory[1..5], &[1, 2, 3, 4]);
        assert_eq!(&stack.memory[8..12], &[1, 2, 3, 4]);
    }

    #[test]
    fn scalar_cache_keeps_single_u64_top_slot_until_spill_boundary() {
        let mut stack = Stack::new(64);
        stack.push_u64(0x1122_3344_5566_7788).unwrap();
        assert_eq!(stack.cache.width, CachedOperandWidth::Eight);
        assert_eq!(stack.committed_top(), 0);
        assert_eq!(stack.peek_top_u64(), 0x1122_3344_5566_7788);

        stack.push_u64(0x99aa_bbcc_ddee_ff00).unwrap();

        assert_eq!(stack.cache.width, CachedOperandWidth::Eight);
        assert_eq!(stack.committed_top(), 8);
        assert_eq!(trusted_read_u64(&stack.memory[0..8]), 0x1122_3344_5566_7788);
        assert_eq!(stack.peek_top_u64(), 0x99aa_bbcc_ddee_ff00);

        stack.push_u64(0x0102_0304_0506_0708).unwrap();

        assert_eq!(stack.cache.width, CachedOperandWidth::Eight);
        assert_eq!(stack.committed_top(), 16);
        assert_eq!(
            trusted_read_u64(&stack.memory[8..16]),
            0x99aa_bbcc_ddee_ff00
        );
        assert_eq!(stack.pop_u64(), 0x0102_0304_0506_0708);
        assert_eq!(stack.pop_u64(), 0x99aa_bbcc_ddee_ff00);
        assert_eq!(stack.pop_u64(), 0x1122_3344_5566_7788);
    }

    #[test]
    fn raw_stack_access_flushes_cached_scalars() {
        let mut stack = Stack::new(64);
        stack.push_u32(0x0102_0304).unwrap();
        assert_eq!(stack.cache.width, CachedOperandWidth::Four);

        stack.push_slice(&[0xaa, 0xbb, 0xcc, 0xdd]).unwrap();

        assert_eq!(stack.cache.width, CachedOperandWidth::None);
        assert_eq!(trusted_read_u32(&stack.memory[0..4]), 0x0102_0304);
        assert_eq!(stack.drop(4), &[0xaa, 0xbb, 0xcc, 0xdd]);
        assert_eq!(stack.pop_u32(), 0x0102_0304);
    }

    #[test]
    fn typed_local_ops_roundtrip_with_cached_top_slots() {
        let mut stack = Stack::new(64);
        let reference = LocalReference::from_raw(0, 16);

        stack.push_slice(&[0; 16]).unwrap();
        stack.push_u64(0x1122_3344_5566_7788).unwrap();
        assert_eq!(stack.cache.width, CachedOperandWidth::Eight);

        stack.local_set8(&reference, 0);
        assert_eq!(trusted_read_u64(&stack.memory[0..8]), 0x1122_3344_5566_7788);

        stack.local_get8(&reference, 0).unwrap();
        assert_eq!(stack.cache.width, CachedOperandWidth::Eight);
        stack.local_tee8(&reference, 8);

        assert_eq!(
            trusted_read_u64(&stack.memory[8..16]),
            0x1122_3344_5566_7788
        );
        assert_eq!(stack.pop_u64(), 0x1122_3344_5566_7788);
    }

    #[test]
    fn pop2_and_replace_top_u32_reuses_remaining_slot() {
        let mut stack = Stack::new(64);
        stack.push_u32(10).unwrap();
        stack.push_u32(20).unwrap();

        let (lhs, rhs) = stack.pop2_u32();
        assert_eq!((lhs, rhs), (10, 20));
        assert_eq!(stack.top, 4);
        assert_eq!(stack.cache.width, CachedOperandWidth::None);

        stack.replace_top_u32(lhs.wrapping_add(rhs));
        assert_eq!(stack.cache.width, CachedOperandWidth::Four);
        assert_eq!(stack.peek_top_u32(), 30);
        assert_eq!(stack.pop_u32(), 30);
    }

    #[test]
    fn pop2_u64_reads_cached_top_and_committed_lower_slot() {
        let mut stack = Stack::new(64);
        stack.push_u64(0x1111_2222_3333_4444).unwrap();
        stack.push_u64(0x5555_6666_7777_8888).unwrap();

        let (lhs, rhs) = stack.pop2_u64();
        assert_eq!((lhs, rhs), (0x1111_2222_3333_4444, 0x5555_6666_7777_8888));
        assert_eq!(stack.top, 8);
        assert_eq!(stack.cache.width, CachedOperandWidth::None);
    }

    #[test]
    fn select_top_u32_works_with_cached_top_and_committed_lower_slot() {
        let mut stack = Stack::new(64);
        stack.push_u32(10).unwrap();
        stack.push_u32(20).unwrap();

        stack.select_top_u32(1);
        assert_eq!(stack.cache.width, CachedOperandWidth::Four);
        assert_eq!(stack.peek_top_u32(), 10);
        assert_eq!(stack.pop_u32(), 10);
    }

    #[test]
    fn replace_top_u64_can_overwrite_committed_top_without_spill() {
        let mut stack = Stack::new(64);
        stack.push_u64(0x1111_2222_3333_4444).unwrap();
        stack.push_slice(&[0xaa, 0xbb, 0xcc, 0xdd]).unwrap();
        stack.drop(4);

        stack.replace_top_u64(0x5555_6666_7777_8888);
        assert_eq!(stack.cache.width, CachedOperandWidth::Eight);
        assert_eq!(stack.peek_top_u64(), 0x5555_6666_7777_8888);
        assert_eq!(stack.pop_u64(), 0x5555_6666_7777_8888);
    }
}
