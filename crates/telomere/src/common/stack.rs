#![allow(private_interfaces)]

#[cfg(feature = "simd")]
use wide::{f32x4, f64x2, i16x8, i32x4, i64x2, i8x16, u16x8, u32x4, u64x2, u8x16};

use crate::VMResult;
use std::fmt::Debug;

use super::{
    memory::trusted_copy_from_slice,
    object_ref::ObjectRef,
    store::{
        FunctionInstanceData, InstanceId, InstanceMemorySlot, LocalMemoryId, MemoryHandle,
        SharedMemoryId,
    },
    Instr, ReturnShape, StablePc, StoreInner,
};
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
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub(crate) struct CallStackInfo {
    return_pc: StablePc,
    prev_local_reference_top: usize,
    prev_local_reference_size: u32,
    code_addr: ObjectRef,
    code_base: *const Instr,
    instance: InstanceId,
    memory0_kind: CachedMemoryKind,
    memory0_raw: u32,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CachedMemoryKind {
    None = 0,
    Local = 1,
    Shared = 2,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CallFrameCache {
    pub(crate) code_addr: ObjectRef,
    pub(crate) code_base: *const Instr,
    pub(crate) instance: InstanceId,
    pub(crate) memory0_kind: CachedMemoryKind,
    pub(crate) memory0_raw: u32,
}

impl CachedMemoryKind {
    fn from_memory_handle(handle: Option<MemoryHandle>) -> (Self, u32) {
        match handle {
            Some(MemoryHandle::Local(id)) => (Self::Local, id.raw()),
            Some(MemoryHandle::Shared(id)) => (Self::Shared, id.raw()),
            None => (Self::None, 0),
        }
    }
}

impl CallFrameCache {
    pub(crate) fn dummy() -> Self {
        Self {
            code_addr: ObjectRef(0),
            code_base: std::ptr::null(),
            instance: InstanceId::from_index(0),
            memory0_kind: CachedMemoryKind::None,
            memory0_raw: 0,
        }
    }

    pub(crate) fn from_parts(
        code_addr: ObjectRef,
        func: &FunctionInstanceData,
        memory0: Option<MemoryHandle>,
    ) -> Self {
        let (memory0_kind, memory0_raw) = CachedMemoryKind::from_memory_handle(memory0);
        Self {
            code_addr,
            code_base: func.canonical_code_pointer().unwrap_or(std::ptr::null()),
            instance: func.instance,
            memory0_kind,
            memory0_raw,
        }
    }

    pub(crate) fn memory0_handle(self) -> Option<MemoryHandle> {
        match self.memory0_kind {
            CachedMemoryKind::None => None,
            CachedMemoryKind::Local => Some(MemoryHandle::Local(LocalMemoryId::from_raw(
                self.memory0_raw,
            ))),
            CachedMemoryKind::Shared => Some(MemoryHandle::Shared(SharedMemoryId::from_raw(
                self.memory0_raw,
            ))),
        }
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
    pub local_size: u32,
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
        self.cache = OperandCache::EMPTY;
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
        self.flush_cached_operands();
        self.top = new_top;
        self.set_cached_operand(CachedOperandWidth::Four, u64::from(v));
        VMResult::Success(())
    }
    #[inline(always)]
    fn push_cached_u64(&mut self, v: u64) -> VMResult<()> {
        let new_top = vm_try!(self.checked_new_top(8));
        self.flush_cached_operands();
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
        if let Some(bits) = self.peek_cached_operand(CachedOperandWidth::Four) {
            self.top -= 4;
            self.cache = OperandCache::EMPTY;
            return bits as u32;
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
        if let Some(bits) = self.peek_cached_operand(CachedOperandWidth::Eight) {
            self.top -= 8;
            self.cache = OperandCache::EMPTY;
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
        self.set_cached_operand(CachedOperandWidth::Four, u64::from(value));
    }
    #[inline(always)]
    pub fn replace_top_u64(&mut self, value: u64) {
        debug_assert!(self.top >= 8);
        debug_assert!(matches!(
            self.cache.width,
            CachedOperandWidth::None | CachedOperandWidth::Eight
        ));
        self.set_cached_operand(CachedOperandWidth::Eight, value);
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
                self.cache = OperandCache::EMPTY;
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
                self.cache = OperandCache::EMPTY;
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
        reference.local_top + reference.local_size as usize + stack_top
    }
    pub fn access_locals(&mut self, reference: &LocalReference) -> &mut [u8] {
        self.flush_cached_operands();
        &mut self.memory[reference.local_top..self.top + reference.local_size as usize]
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
    fn call_stack_info(&self, reference: &LocalReference) -> CallStackInfo {
        let info_top = reference.local_top + reference.local_size as usize
            - std::mem::size_of::<CallStackInfo>();

        unsafe {
            std::ptr::read_unaligned(
                self.memory[info_top..info_top + std::mem::size_of::<CallStackInfo>()]
                    .as_ptr()
                    .cast::<CallStackInfo>(),
            )
        }
    }
    fn push_call_stack_info(&mut self, info: CallStackInfo) -> VMResult<()> {
        self.flush_cached_operands();
        let size = std::mem::size_of::<CallStackInfo>();
        let bytes = unsafe {
            std::slice::from_raw_parts((&info as *const CallStackInfo).cast::<u8>(), size)
        };
        trusted_copy_from_slice(vm_try!(self.get_memory(size)), bytes);
        self.add_top(size)
    }
    pub(crate) fn previous_local_reference(&self, reference: &LocalReference) -> LocalReference {
        let CallStackInfo {
            prev_local_reference_top,
            prev_local_reference_size,
            ..
        } = self.call_stack_info(reference);
        LocalReference {
            local_top: prev_local_reference_top,
            local_size: prev_local_reference_size,
        }
    }
    pub fn code_addr(&self, reference: &LocalReference) -> ObjectRef {
        self.call_stack_info(reference).code_addr
    }
    pub fn code_base(&self, reference: &LocalReference) -> *const Instr {
        self.call_stack_info(reference).code_base
    }
    pub(crate) fn frame_cache(&self, reference: &LocalReference) -> CallFrameCache {
        let info = self.call_stack_info(reference);
        CallFrameCache {
            code_addr: info.code_addr,
            code_base: info.code_base,
            instance: info.instance,
            memory0_kind: info.memory0_kind,
            memory0_raw: info.memory0_raw,
        }
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
        self.flush_cached_operands();
        let frame = frame.into_call_frame_cache(runtime);
        let local_top = vm_try!(VMResult::from_option(
            self.top.checked_sub(param_size),
            || VMResult::StackOverflow
        ));

        vm_try!(self.add_top(local_size));
        vm_try!(self.zero_new_locals(local_top + param_size, local_size));
        let info = CallStackInfo {
            return_pc: StablePc::from_raw_in_frame(
                runtime,
                self,
                prev_local_reference,
                return_addr,
            ),
            code_addr: frame.code_addr,
            code_base: frame.code_base,
            instance: frame.instance,
            memory0_kind: frame.memory0_kind,
            memory0_raw: frame.memory0_raw,
            prev_local_reference_top: prev_local_reference.local_top,
            prev_local_reference_size: prev_local_reference.local_size,
        };
        vm_try!(self.push_call_stack_info(info));

        VMResult::Success(LocalReference {
            local_top,
            local_size: (param_size + local_size + std::mem::size_of::<CallStackInfo>()) as u32,
        })
    }
    pub fn function_return(
        &mut self,
        reference: &LocalReference,
        return_size: usize,
        runtime: &StoreInner,
    ) -> (LocalReference, *const Instr) {
        self.flush_cached_operands();
        let CallStackInfo {
            return_pc,
            prev_local_reference_top,
            prev_local_reference_size,
            ..
        } = self.call_stack_info(reference);
        let prev_local_reference = LocalReference {
            local_size: prev_local_reference_size,
            local_top: prev_local_reference_top,
        };

        self.memory
            .copy_within(self.top - return_size..self.top, reference.local_top);
        self.top = reference.local_top + return_size;
        (
            prev_local_reference,
            return_pc.resolve(runtime, self, prev_local_reference),
        )
    }

    pub fn function_return_empty(
        &mut self,
        reference: &LocalReference,
        runtime: &StoreInner,
    ) -> (LocalReference, *const Instr) {
        self.flush_cached_operands();
        let CallStackInfo {
            return_pc,
            prev_local_reference_top,
            prev_local_reference_size,
            ..
        } = self.call_stack_info(reference);
        let prev_local_reference = LocalReference {
            local_size: prev_local_reference_size,
            local_top: prev_local_reference_top,
        };
        self.cache = OperandCache::EMPTY;
        self.top = reference.local_top;
        (
            prev_local_reference,
            return_pc.resolve(runtime, self, prev_local_reference),
        )
    }

    pub fn function_return4(
        &mut self,
        reference: &LocalReference,
        runtime: &StoreInner,
    ) -> (LocalReference, *const Instr) {
        let CallStackInfo {
            return_pc,
            prev_local_reference_top,
            prev_local_reference_size,
            ..
        } = self.call_stack_info(reference);
        let prev_local_reference = LocalReference {
            local_size: prev_local_reference_size,
            local_top: prev_local_reference_top,
        };
        let value = match self.cache.width {
            CachedOperandWidth::Four => self.cache.bits as u32,
            CachedOperandWidth::None => trusted_read_u32(&self.memory[self.top - 4..self.top]),
            CachedOperandWidth::Eight => unreachable!("validated 4-byte function return"),
        };
        self.top = reference.local_top + 4;
        self.set_cached_operand(CachedOperandWidth::Four, u64::from(value));
        (
            prev_local_reference,
            return_pc.resolve(runtime, self, prev_local_reference),
        )
    }

    pub fn function_return8(
        &mut self,
        reference: &LocalReference,
        runtime: &StoreInner,
    ) -> (LocalReference, *const Instr) {
        let CallStackInfo {
            return_pc,
            prev_local_reference_top,
            prev_local_reference_size,
            ..
        } = self.call_stack_info(reference);
        let prev_local_reference = LocalReference {
            local_size: prev_local_reference_size,
            local_top: prev_local_reference_top,
        };
        let value = match self.cache.width {
            CachedOperandWidth::Eight => self.cache.bits,
            CachedOperandWidth::None => trusted_read_u64(&self.memory[self.top - 8..self.top]),
            CachedOperandWidth::Four => unreachable!("validated 8-byte function return"),
        };
        self.top = reference.local_top + 8;
        self.set_cached_operand(CachedOperandWidth::Eight, value);
        (
            prev_local_reference,
            return_pc.resolve(runtime, self, prev_local_reference),
        )
    }

    pub fn function_return_shaped(
        &mut self,
        reference: &LocalReference,
        return_size: usize,
        shape: ReturnShape,
        runtime: &StoreInner,
    ) -> (LocalReference, *const Instr) {
        match shape {
            ReturnShape::Empty => self.function_return_empty(reference, runtime),
            ReturnShape::Scalar4 => self.function_return4(reference, runtime),
            ReturnShape::Scalar8 => self.function_return8(reference, runtime),
            ReturnShape::Generic => self.function_return(reference, return_size, runtime),
        }
    }
    /// Like `function_return` but assumes the return data is already written at `local_top`.
    pub fn function_return_in_place(
        &mut self,
        reference: &LocalReference,
        return_size: usize,
        runtime: &StoreInner,
    ) -> (LocalReference, *const Instr) {
        self.function_return_in_place_shaped(
            reference,
            return_size,
            ReturnShape::from_size(return_size as u32),
            runtime,
        )
    }

    pub fn function_return_in_place_shaped(
        &mut self,
        reference: &LocalReference,
        return_size: usize,
        _shape: ReturnShape,
        runtime: &StoreInner,
    ) -> (LocalReference, *const Instr) {
        self.flush_cached_operands();
        let CallStackInfo {
            return_pc,
            prev_local_reference_top,
            prev_local_reference_size,
            ..
        } = self.call_stack_info(reference);

        let prev_local_reference = LocalReference {
            local_size: prev_local_reference_size,
            local_top: prev_local_reference_top,
        };
        self.cache = OperandCache::EMPTY;
        self.top = reference.local_top + return_size;
        (
            prev_local_reference,
            return_pc.resolve(runtime, self, prev_local_reference),
        )
    }
    pub fn function_return_call(
        &mut self,
        reference: &LocalReference,
        param_size: usize,
        param_shape: ReturnShape,
        local_size: usize,
        frame: CallFrameCache,
    ) -> VMResult<LocalReference> {
        self.flush_cached_operands();
        tracing::trace!("function_return_call: {reference:?}");
        let CallStackInfo {
            return_pc,
            prev_local_reference_top,
            prev_local_reference_size,
            ..
        } = self.call_stack_info(reference);
        self.move_top_value_to_dst(reference.local_top, param_size, param_shape);
        vm_try!(self.add_top(local_size));
        vm_try!(self.zero_new_locals(reference.local_top + param_size, local_size));

        let info = CallStackInfo {
            return_pc,
            code_addr: frame.code_addr,
            code_base: frame.code_base,
            instance: frame.instance,
            memory0_kind: frame.memory0_kind,
            memory0_raw: frame.memory0_raw,
            prev_local_reference_top,
            prev_local_reference_size,
        };
        vm_try!(self.push_call_stack_info(info));

        VMResult::Success(LocalReference {
            local_top: reference.local_top,
            local_size: (param_size + local_size + std::mem::size_of::<CallStackInfo>()) as u32,
        })
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

    fn frame(kind: CachedMemoryKind, raw: u32) -> CallFrameCache {
        CallFrameCache {
            code_addr: ObjectRef(0),
            code_base: std::ptr::null(),
            instance: InstanceId::from_index(0),
            memory0_kind: kind,
            memory0_raw: raw,
        }
    }

    #[test]
    fn function_call_and_return_roundtrip_result_bytes() {
        let mut stack = Stack::new(256);
        let runtime = StoreInner::new();
        let prev = LocalReference {
            local_top: 0,
            local_size: 0,
        };

        stack.push_u32(0x1122_3344).unwrap();
        let reference = stack
            .function_call(
                4,
                8,
                frame(CachedMemoryKind::Local, 9),
                prev,
                std::ptr::null(),
                &runtime,
            )
            .unwrap();

        let reference_local_top = reference.local_top;
        let reference_local_size = reference.local_size;
        assert_eq!(reference_local_top, 0);
        assert_eq!(
            reference_local_size as usize,
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
        let restored_local_size = restored.local_size;
        assert_eq!(restored_local_top, 0);
        assert_eq!(restored_local_size, 0);
        assert!(return_pc.is_null());
        assert_eq!(stack.top, 4);
        assert_eq!(stack.pop_u32(), 0xaabb_ccdd);
    }

    #[test]
    fn function_return_in_place_uses_local_area_as_result_slot() {
        let mut stack = Stack::new(256);
        let runtime = StoreInner::new();
        let prev = LocalReference {
            local_top: 0,
            local_size: 0,
        };

        let reference = stack
            .function_call(
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
        let restored_local_size = restored.local_size;
        assert_eq!(restored_local_top, 0);
        assert_eq!(restored_local_size, 0);
        assert!(return_pc.is_null());
        assert_eq!(stack.top, 4);
        assert_eq!(stack.pop_u32(), 0x5566_7788);
    }

    #[test]
    fn function_return_fast_path_keeps_u32_result_cached() {
        let mut stack = Stack::new(256);
        let runtime = StoreInner::new();
        let prev = LocalReference {
            local_top: 0,
            local_size: 0,
        };

        stack.push_u32(0x0102_0304).unwrap();
        let reference = stack
            .function_call(
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
        let restored_local_size = restored.local_size;
        assert_eq!(restored_local_top, 0);
        assert_eq!(restored_local_size, 0);
        assert!(return_pc.is_null());
        assert_eq!(stack.pop_u32(), 0xaabb_ccdd);
    }

    #[test]
    fn function_return_call_reuses_frame_slot_and_zeroes_new_locals() {
        let mut stack = Stack::new(256);
        let runtime = StoreInner::new();
        let empty = LocalReference {
            local_top: 0,
            local_size: 0,
        };
        let caller = stack
            .function_call(
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
            .function_call(
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
            .function_return_call(
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
            tail.local_size as usize,
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
    fn block_return_moves_result_to_block_stack_top() {
        let mut stack = Stack::new(256);
        let runtime = StoreInner::new();
        let empty = LocalReference {
            local_top: 0,
            local_size: 0,
        };
        let reference = stack
            .function_call(
                0,
                4,
                frame(CachedMemoryKind::Local, 1),
                empty,
                std::ptr::null(),
                &runtime,
            )
            .unwrap();
        let frame_top = reference.local_top + reference.local_size as usize;
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
        let empty = LocalReference {
            local_top: 0,
            local_size: 0,
        };
        let reference = stack
            .function_call(
                0,
                4,
                frame(CachedMemoryKind::Local, 1),
                empty,
                std::ptr::null(),
                &runtime,
            )
            .unwrap();
        let frame_top = reference.local_top + reference.local_size as usize;

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
        let empty = LocalReference {
            local_top: 0,
            local_size: 0,
        };
        let reference = stack
            .function_call(
                0,
                8,
                frame(CachedMemoryKind::Local, 1),
                empty,
                std::ptr::null(),
                &runtime,
            )
            .unwrap();
        let frame_top = reference.local_top + reference.local_size as usize;

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
        let empty = LocalReference {
            local_top: 0,
            local_size: 0,
        };
        let reference = stack
            .function_call(
                0,
                4,
                frame(CachedMemoryKind::Local, 1),
                empty,
                std::ptr::null(),
                &runtime,
            )
            .unwrap();
        let frame_top = reference.local_top + reference.local_size as usize;

        stack.push_u32(0xaabb_ccdd).unwrap();
        stack.block_return(&reference, 0, 0);

        assert_eq!(stack.top, frame_top);
        assert_eq!(stack.cache.width, CachedOperandWidth::None);
    }

    #[test]
    fn generic_local_get_copies_bytes_to_operand_stack() {
        let mut stack = Stack::new(64);
        let reference = LocalReference {
            local_top: 0,
            local_size: 8,
        };

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
        let reference = LocalReference {
            local_top: 0,
            local_size: 8,
        };

        stack.push_slice(&[0; 8]).unwrap();
        stack.push_slice(&[0xaa, 0xbb, 0xcc, 0xdd]).unwrap();
        stack.local_set(&reference, 2, 4);

        assert_eq!(stack.top, 8);
        assert_eq!(&stack.memory[2..6], &[0xaa, 0xbb, 0xcc, 0xdd]);
    }

    #[test]
    fn generic_local_tee_keeps_operand_stack_and_updates_local_slot() {
        let mut stack = Stack::new(64);
        let reference = LocalReference {
            local_top: 0,
            local_size: 8,
        };

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

        assert_eq!(trusted_read_u64(&stack.memory[0..8]), 0x1122_3344_5566_7788);
        assert_eq!(stack.cache.width, CachedOperandWidth::Eight);
        assert_eq!(stack.committed_top(), 8);
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
        let reference = LocalReference {
            local_top: 0,
            local_size: 16,
        };

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
