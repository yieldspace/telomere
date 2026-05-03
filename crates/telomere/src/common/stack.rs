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
    Instr, StablePc, StoreInner,
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
}
impl Debug for Stack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Stack({},{:?})", self.top, &self.memory[0..self.top])
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

const CALL_STACK_INFO_SIZE: usize = std::mem::size_of::<CallStackInfo>();

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
unsafe impl Send for CallFrameCache {}
unsafe impl Sync for CallFrameCache {}

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
            code_base: func.code_pointer().unwrap_or(std::ptr::null()),
            instance: func.instance,
            memory0_kind,
            memory0_raw,
        }
    }

    pub(crate) fn from_cached_parts(
        code_addr: ObjectRef,
        instance: InstanceId,
        code_base: *const Instr,
        memory0: Option<MemoryHandle>,
    ) -> Self {
        let (memory0_kind, memory0_raw) = CachedMemoryKind::from_memory_handle(memory0);
        Self {
            code_addr,
            code_base,
            instance,
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
    #[inline(always)]
    unsafe fn local_ptr(local_base: *const u8, local_addr: usize) -> *const u8 {
        unsafe { local_base.add(local_addr) }
    }

    #[inline(always)]
    unsafe fn local_mut_ptr(local_base: *mut u8, local_addr: usize) -> *mut u8 {
        unsafe { local_base.add(local_addr) }
    }

    pub fn new(size: usize) -> Self {
        let vec = vec![0; size];
        Stack {
            memory: vec.into_boxed_slice(),
            top: 0,
        }
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
        trusted_copy_from_slice(vm_try!(self.get_memory(N)), &v);
        self.add_top(N)
    }

    /// # Safety
    /// Caller must ensure `src..src+N` is readable for the duration of the copy.
    #[inline(always)]
    pub unsafe fn push_copy_from_ptr<const N: usize>(&mut self, src: *const u8) -> VMResult<()> {
        let new_top = vm_try!(VMResult::from_option(self.top.checked_add(N), || {
            VMResult::StackOverflow
        }));
        if new_top > self.memory.len() {
            return VMResult::StackOverflow;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(src, self.memory.as_mut_ptr().add(self.top), N);
        }
        self.top = new_top;
        VMResult::Success(())
    }

    pub fn push_slice(&mut self, v: &[u8]) -> VMResult<()> {
        trusted_copy_from_slice(vm_try!(self.get_memory(v.len())), v);
        self.add_top(v.len())
    }
    pub fn pop_u8_array<const N: usize>(&mut self) -> [u8; N] {
        self.sub_top(N);
        let mut arr = [0u8; N];
        trusted_copy_from_slice(&mut arr, &self.memory[self.top..self.top + N]);
        arr
    }
    pub fn pop_u8_array_generic<const N: usize>(&mut self, n: usize) -> [u8; N] {
        self.sub_top(n);

        let mut arr = [0u8; N];
        trusted_copy_from_slice(&mut arr, &self.memory[self.top..self.top + N]);
        arr
    }
    pub fn drop(&mut self, n: usize) -> &[u8] {
        self.sub_top(n);

        (&self.memory[self.top..self.top + n]) as _
    }
    pub fn push_u32(&mut self, v: u32) -> VMResult<()> {
        trusted_write_u32(vm_try!(self.get_memory(4)), v);
        self.add_top(4)
    }

    #[inline(always)]
    pub fn push_u32_fast(&mut self, v: u32) -> VMResult<()> {
        let new_top = vm_try!(VMResult::from_option(self.top.checked_add(4), || {
            VMResult::StackOverflow
        }));
        if new_top > self.memory.len() {
            return VMResult::StackOverflow;
        }
        unsafe {
            self.memory
                .as_mut_ptr()
                .add(self.top)
                .cast::<u32>()
                .write_unaligned(v.to_le());
        }
        self.top = new_top;
        VMResult::Success(())
    }

    pub fn pop_u32(&mut self) -> u32 {
        self.sub_top(4);
        trusted_read_u32(&self.memory[self.top..self.top + 4])
    }

    #[inline(always)]
    pub fn pop_u32_fast(&mut self) -> u32 {
        self.sub_top(4);
        u32::from_le(unsafe {
            self.memory
                .as_ptr()
                .add(self.top)
                .cast::<u32>()
                .read_unaligned()
        })
    }

    #[inline(always)]
    pub fn reduce_top_i32_add(&mut self) -> i32 {
        debug_assert!(self.top >= 8);
        let rhs_offset = self.top - 4;
        let lhs_offset = self.top - 8;
        let lhs = u32::from_le(unsafe {
            self.memory
                .as_ptr()
                .add(lhs_offset)
                .cast::<u32>()
                .read_unaligned()
        });
        let rhs = u32::from_le(unsafe {
            self.memory
                .as_ptr()
                .add(rhs_offset)
                .cast::<u32>()
                .read_unaligned()
        });
        let result = (lhs as i32).wrapping_add(rhs as i32);
        unsafe {
            self.memory
                .as_mut_ptr()
                .add(lhs_offset)
                .cast::<u32>()
                .write_unaligned((result as u32).to_le());
        }
        self.top -= 4;
        result
    }

    #[inline(always)]
    pub fn reduce_top_i32_sub(&mut self) -> i32 {
        debug_assert!(self.top >= 8);
        let rhs_offset = self.top - 4;
        let lhs_offset = self.top - 8;
        let lhs = u32::from_le(unsafe {
            self.memory
                .as_ptr()
                .add(lhs_offset)
                .cast::<u32>()
                .read_unaligned()
        });
        let rhs = u32::from_le(unsafe {
            self.memory
                .as_ptr()
                .add(rhs_offset)
                .cast::<u32>()
                .read_unaligned()
        });
        let result = (lhs as i32).wrapping_sub(rhs as i32);
        unsafe {
            self.memory
                .as_mut_ptr()
                .add(lhs_offset)
                .cast::<u32>()
                .write_unaligned((result as u32).to_le());
        }
        self.top -= 4;
        result
    }

    pub fn push_u64(&mut self, v: u64) -> VMResult<()> {
        trusted_write_u64(vm_try!(self.get_memory(8)), v);
        self.add_top(8)
    }

    #[inline(always)]
    pub fn push_u64_fast(&mut self, v: u64) -> VMResult<()> {
        let new_top = vm_try!(VMResult::from_option(self.top.checked_add(8), || {
            VMResult::StackOverflow
        }));
        if new_top > self.memory.len() {
            return VMResult::StackOverflow;
        }
        unsafe {
            self.memory
                .as_mut_ptr()
                .add(self.top)
                .cast::<u64>()
                .write_unaligned(v.to_le());
        }
        self.top = new_top;
        VMResult::Success(())
    }

    pub fn push_u128(&mut self, v: u128) -> VMResult<()> {
        trusted_write_u128(vm_try!(self.get_memory(16)), v);
        self.add_top(16)
    }

    #[inline(always)]
    pub fn push_u128_fast(&mut self, v: u128) -> VMResult<()> {
        let new_top = vm_try!(VMResult::from_option(self.top.checked_add(16), || {
            VMResult::StackOverflow
        }));
        if new_top > self.memory.len() {
            return VMResult::StackOverflow;
        }
        unsafe {
            self.memory
                .as_mut_ptr()
                .add(self.top)
                .cast::<u128>()
                .write_unaligned(v.to_le());
        }
        self.top = new_top;
        VMResult::Success(())
    }
    pub fn pop_u128(&mut self) -> u128 {
        self.sub_top(16);
        trusted_read_u128(&self.memory[self.top..self.top + 16])
    }

    #[inline(always)]
    pub fn pop_u128_fast(&mut self) -> u128 {
        self.sub_top(16);
        u128::from_le(unsafe {
            self.memory
                .as_ptr()
                .add(self.top)
                .cast::<u128>()
                .read_unaligned()
        })
    }
    pub fn pop_u64(&mut self) -> u64 {
        self.sub_top(8);
        trusted_read_u64(&self.memory[self.top..self.top + 8])
    }

    #[inline(always)]
    pub fn pop_u64_fast(&mut self) -> u64 {
        self.sub_top(8);
        u64::from_le(unsafe {
            self.memory
                .as_ptr()
                .add(self.top)
                .cast::<u64>()
                .read_unaligned()
        })
    }
    pub fn push_i32(&mut self, v: i32) -> VMResult<()> {
        self.push_u32(v as u32)
    }

    #[inline(always)]
    pub fn push_i32_fast(&mut self, v: i32) -> VMResult<()> {
        self.push_u32_fast(v as u32)
    }
    #[inline(always)]
    pub fn push_i64_fast(&mut self, v: i64) -> VMResult<()> {
        self.push_u64_fast(v as u64)
    }
    #[inline(always)]
    pub fn push_f32_fast(&mut self, v: f32) -> VMResult<()> {
        self.push_u32_fast(v.to_bits())
    }
    #[inline(always)]
    pub fn push_f64_fast(&mut self, v: f64) -> VMResult<()> {
        self.push_u64_fast(v.to_bits())
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
    pub fn access_locals(&mut self, reference: &LocalReference) -> &mut [u8] {
        &mut self.memory[reference.local_top..self.top + reference.local_size as usize]
    }
    pub fn local_get(
        &mut self,
        reference: &LocalReference,
        local_addr: usize,
        size: usize,
    ) -> VMResult<()> {
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
        let local_base = unsafe { self.local_area_mut_ptr(reference) };
        unsafe { self.local_get4_from_base(local_base, local_addr) }
    }

    /// # Safety
    /// Caller must ensure `local_base` points at the active locals area for the current frame and
    /// that `local_addr..local_addr+4` is a valid local slot.
    #[inline(always)]
    pub unsafe fn local_get4_from_base(
        &mut self,
        local_base: *const u8,
        local_addr: usize,
    ) -> VMResult<()> {
        let new_top = vm_try!(VMResult::from_option(self.top.checked_add(4), || {
            VMResult::StackOverflow
        }));
        if new_top > self.memory.len() {
            return VMResult::StackOverflow;
        }
        let value = u32::from_le(unsafe {
            Self::local_ptr(local_base, local_addr)
                .cast::<u32>()
                .read_unaligned()
        });
        trusted_write_u32(&mut self.memory[self.top..new_top], value);
        self.top = new_top;
        VMResult::Success(())
    }

    #[inline(always)]
    pub(crate) unsafe fn local_u32_from_base(
        &self,
        local_base: *const u8,
        local_addr: usize,
    ) -> u32 {
        u32::from_le(unsafe {
            Self::local_ptr(local_base, local_addr)
                .cast::<u32>()
                .read_unaligned()
        })
    }
    #[inline(always)]
    pub fn local_get8(&mut self, reference: &LocalReference, local_addr: usize) -> VMResult<()> {
        let local_base = unsafe { self.local_area_mut_ptr(reference) };
        unsafe { self.local_get8_from_base(local_base, local_addr) }
    }

    /// # Safety
    /// Caller must ensure `local_base` points at the active locals area for the current frame and
    /// that `local_addr..local_addr+8` is a valid local slot.
    #[inline(always)]
    pub unsafe fn local_get8_from_base(
        &mut self,
        local_base: *const u8,
        local_addr: usize,
    ) -> VMResult<()> {
        let new_top = vm_try!(VMResult::from_option(self.top.checked_add(8), || {
            VMResult::StackOverflow
        }));
        if new_top > self.memory.len() {
            return VMResult::StackOverflow;
        }
        let value = u64::from_le(unsafe {
            Self::local_ptr(local_base, local_addr)
                .cast::<u64>()
                .read_unaligned()
        });
        trusted_write_u64(&mut self.memory[self.top..new_top], value);
        self.top = new_top;
        VMResult::Success(())
    }
    #[inline(always)]
    pub fn local_get16(&mut self, reference: &LocalReference, local_addr: usize) -> VMResult<()> {
        let local_base = unsafe { self.local_area_mut_ptr(reference) };
        unsafe { self.local_get16_from_base(local_base, local_addr) }
    }

    /// # Safety
    /// Caller must ensure `local_base` points at the active locals area for the current frame and
    /// that `local_addr..local_addr+16` is a valid local slot.
    #[inline(always)]
    pub unsafe fn local_get16_from_base(
        &mut self,
        local_base: *const u8,
        local_addr: usize,
    ) -> VMResult<()> {
        let new_top = vm_try!(VMResult::from_option(self.top.checked_add(16), || {
            VMResult::StackOverflow
        }));
        if new_top > self.memory.len() {
            return VMResult::StackOverflow;
        }
        let value = u128::from_le(unsafe {
            Self::local_ptr(local_base, local_addr)
                .cast::<u128>()
                .read_unaligned()
        });
        trusted_write_u128(&mut self.memory[self.top..new_top], value);
        self.top = new_top;
        VMResult::Success(())
    }
    pub fn local_set(&mut self, reference: &LocalReference, local_addr: usize, size: usize) {
        self.top -= size;
        self.memory
            .copy_within(self.top..self.top + size, reference.local_top + local_addr);
    }
    #[inline(always)]
    pub fn local_set4(&mut self, reference: &LocalReference, local_addr: usize) {
        let local_base = unsafe { self.local_area_mut_ptr(reference) };
        unsafe { self.local_set4_from_base(local_base, local_addr) }
    }

    /// # Safety
    /// Caller must ensure `local_base` points at the active locals area for the current frame and
    /// that `local_addr..local_addr+4` is a valid writable local slot.
    #[inline(always)]
    pub unsafe fn local_set4_from_base(&mut self, local_base: *mut u8, local_addr: usize) {
        let value = self.pop_u32_fast();
        unsafe {
            Self::local_mut_ptr(local_base, local_addr)
                .cast::<u32>()
                .write_unaligned(value.to_le());
        }
    }

    /// # Safety
    /// Caller must ensure `local_base` points at the active locals area for the current frame and
    /// that `local_addr..local_addr+4` is a valid writable local slot.
    #[inline(always)]
    pub unsafe fn local_set4_from_base_value(
        &mut self,
        local_base: *mut u8,
        local_addr: usize,
        value: u32,
    ) {
        unsafe {
            Self::local_mut_ptr(local_base, local_addr)
                .cast::<u32>()
                .write_unaligned(value.to_le());
        }
    }
    #[inline(always)]
    pub fn local_set8(&mut self, reference: &LocalReference, local_addr: usize) {
        let local_base = unsafe { self.local_area_mut_ptr(reference) };
        unsafe { self.local_set8_from_base(local_base, local_addr) }
    }

    /// # Safety
    /// Caller must ensure `local_base` points at the active locals area for the current frame and
    /// that `local_addr..local_addr+8` is a valid writable local slot.
    #[inline(always)]
    pub unsafe fn local_set8_from_base(&mut self, local_base: *mut u8, local_addr: usize) {
        let value = self.pop_u64();
        unsafe {
            Self::local_mut_ptr(local_base, local_addr)
                .cast::<u64>()
                .write_unaligned(value.to_le());
        }
    }
    #[inline(always)]
    pub fn local_set16(&mut self, reference: &LocalReference, local_addr: usize) {
        let local_base = unsafe { self.local_area_mut_ptr(reference) };
        unsafe { self.local_set16_from_base(local_base, local_addr) }
    }

    /// # Safety
    /// Caller must ensure `local_base` points at the active locals area for the current frame and
    /// that `local_addr..local_addr+16` is a valid writable local slot.
    #[inline(always)]
    pub unsafe fn local_set16_from_base(&mut self, local_base: *mut u8, local_addr: usize) {
        let value = self.pop_u128();
        unsafe {
            Self::local_mut_ptr(local_base, local_addr)
                .cast::<u128>()
                .write_unaligned(value.to_le());
        }
    }
    pub fn local_bytes(&self, reference: &LocalReference, local_addr: usize, size: usize) -> &[u8] {
        &self.memory[reference.local_top + local_addr..reference.local_top + local_addr + size]
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
        self.memory.as_mut_ptr().add(reference.local_top)
    }
    pub fn local_tee(&mut self, reference: &LocalReference, local_addr: usize, size: usize) {
        self.memory
            .copy_within(self.top - size..self.top, reference.local_top + local_addr);
    }
    #[inline(always)]
    pub fn local_tee4(&mut self, reference: &LocalReference, local_addr: usize) {
        let local_base = unsafe { self.local_area_mut_ptr(reference) };
        unsafe { self.local_tee4_from_base(local_base, local_addr) }
    }

    /// # Safety
    /// Caller must ensure `local_base` points at the active locals area for the current frame and
    /// that `local_addr..local_addr+4` is a valid writable local slot.
    #[inline(always)]
    pub unsafe fn local_tee4_from_base(&mut self, local_base: *mut u8, local_addr: usize) {
        let value = u32::from_le(unsafe {
            self.memory
                .as_ptr()
                .add(self.top - 4)
                .cast::<u32>()
                .read_unaligned()
        });
        unsafe {
            Self::local_mut_ptr(local_base, local_addr)
                .cast::<u32>()
                .write_unaligned(value.to_le());
        }
    }
    #[inline(always)]
    pub fn local_tee8(&mut self, reference: &LocalReference, local_addr: usize) {
        let local_base = unsafe { self.local_area_mut_ptr(reference) };
        unsafe { self.local_tee8_from_base(local_base, local_addr) }
    }

    /// # Safety
    /// Caller must ensure `local_base` points at the active locals area for the current frame and
    /// that `local_addr..local_addr+8` is a valid writable local slot.
    #[inline(always)]
    pub unsafe fn local_tee8_from_base(&mut self, local_base: *mut u8, local_addr: usize) {
        let value = trusted_read_u64(&self.memory[self.top - 8..self.top]);
        unsafe {
            Self::local_mut_ptr(local_base, local_addr)
                .cast::<u64>()
                .write_unaligned(value.to_le());
        }
    }
    #[inline(always)]
    pub fn local_tee16(&mut self, reference: &LocalReference, local_addr: usize) {
        let local_base = unsafe { self.local_area_mut_ptr(reference) };
        unsafe { self.local_tee16_from_base(local_base, local_addr) }
    }

    /// # Safety
    /// Caller must ensure `local_base` points at the active locals area for the current frame and
    /// that `local_addr..local_addr+16` is a valid writable local slot.
    #[inline(always)]
    pub unsafe fn local_tee16_from_base(&mut self, local_base: *mut u8, local_addr: usize) {
        let value = trusted_read_u128(&self.memory[self.top - 16..self.top]);
        unsafe {
            Self::local_mut_ptr(local_base, local_addr)
                .cast::<u128>()
                .write_unaligned(value.to_le());
        }
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
    #[inline(always)]
    fn write_call_stack_info_at(&mut self, info_top: usize, info: CallStackInfo) {
        unsafe {
            self.memory
                .as_mut_ptr()
                .add(info_top)
                .cast::<CallStackInfo>()
                .write_unaligned(info);
        }
    }
    fn frame_end_from_local_top(
        &self,
        local_top: usize,
        param_size: usize,
        local_size: usize,
    ) -> VMResult<usize> {
        let info_top = vm_try!(VMResult::from_option(
            local_top
                .checked_add(param_size)
                .and_then(|value| value.checked_add(local_size)),
            || VMResult::StackOverflow
        ));
        let new_top = vm_try!(VMResult::from_option(
            info_top.checked_add(CALL_STACK_INFO_SIZE),
            || VMResult::StackOverflow
        ));
        if new_top > self.memory.len() {
            return VMResult::StackOverflow;
        }
        VMResult::Success(new_top)
    }
    fn copy_top_bytes_to(&mut self, dst: usize, size: usize) -> VMResult<()> {
        let src = vm_try!(VMResult::from_option(self.top.checked_sub(size), || {
            VMResult::StackOverflow
        }));
        if size == 0 || src == dst {
            return VMResult::Success(());
        }
        match size {
            4 => {
                let value = trusted_read_u32(&self.memory[src..src + 4]);
                trusted_write_u32(&mut self.memory[dst..dst + 4], value);
                VMResult::Success(())
            }
            8 => {
                let value = trusted_read_u64(&self.memory[src..src + 8]);
                trusted_write_u64(&mut self.memory[dst..dst + 8], value);
                VMResult::Success(())
            }
            16 => {
                let value = trusted_read_u128(&self.memory[src..src + 16]);
                trusted_write_u128(&mut self.memory[dst..dst + 16], value);
                VMResult::Success(())
            }
            _ => {
                self.memory.copy_within(src..src + size, dst);
                VMResult::Success(())
            }
        }
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
        let frame = frame.into_call_frame_cache(runtime);
        let return_pc =
            StablePc::from_raw_in_frame(runtime, self, prev_local_reference, return_addr);
        self.function_call_cached(
            param_size,
            local_size,
            frame,
            prev_local_reference,
            return_pc,
        )
    }

    pub fn function_call_cached(
        &mut self,
        param_size: usize,
        local_size: usize,
        frame: CallFrameCache,
        prev_local_reference: LocalReference,
        return_pc: StablePc,
    ) -> VMResult<LocalReference> {
        let local_top = vm_try!(VMResult::from_option(
            self.top.checked_sub(param_size),
            || VMResult::StackOverflow
        ));
        let new_top = vm_try!(self.frame_end_from_local_top(local_top, param_size, local_size));
        vm_try!(self.zero_new_locals(local_top + param_size, local_size));
        self.write_call_stack_info_at(
            local_top + param_size + local_size,
            CallStackInfo {
                return_pc,
                code_addr: frame.code_addr,
                code_base: frame.code_base,
                instance: frame.instance,
                memory0_kind: frame.memory0_kind,
                memory0_raw: frame.memory0_raw,
                prev_local_reference_top: prev_local_reference.local_top,
                prev_local_reference_size: prev_local_reference.local_size,
            },
        );
        self.top = new_top;

        VMResult::Success(LocalReference {
            local_top,
            local_size: (new_top - local_top) as u32,
        })
    }
    pub fn function_return(
        &mut self,
        reference: &LocalReference,
        return_size: usize,
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
        match self.copy_top_bytes_to(reference.local_top, return_size) {
            VMResult::Success(()) => {}
            VMResult::StackOverflow => unreachable!("validated function return must fit in stack"),
            other => unreachable!("unexpected return copy failure: {other:?}"),
        }
        self.top = reference.local_top + return_size;
        (
            prev_local_reference,
            return_pc.resolve(runtime, self, prev_local_reference),
        )
    }
    /// Like `function_return` but assumes the return data is already written at `local_top`.
    pub fn function_return_in_place(
        &mut self,
        reference: &LocalReference,
        return_size: usize,
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
        local_size: usize,
        frame: CallFrameCache,
    ) -> VMResult<LocalReference> {
        self.function_return_call_cached(reference, param_size, local_size, frame)
    }

    pub fn function_return_call_cached(
        &mut self,
        reference: &LocalReference,
        param_size: usize,
        local_size: usize,
        frame: CallFrameCache,
    ) -> VMResult<LocalReference> {
        tracing::trace!("function_return_call: {reference:?}");
        let CallStackInfo {
            return_pc,
            prev_local_reference_top,
            prev_local_reference_size,
            ..
        } = self.call_stack_info(reference);
        vm_try!(self.copy_top_bytes_to(reference.local_top, param_size));
        let new_top =
            vm_try!(self.frame_end_from_local_top(reference.local_top, param_size, local_size));
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
        self.write_call_stack_info_at(reference.local_top + param_size + local_size, info);
        self.top = new_top;

        VMResult::Success(LocalReference {
            local_top: reference.local_top,
            local_size: (new_top - reference.local_top) as u32,
        })
    }
    pub fn block_return(
        &mut self,
        reference: &LocalReference,
        stack_top: usize,
        return_size: usize,
    ) {
        let dst = reference.local_top + reference.local_size as usize + stack_top;
        match self.copy_top_bytes_to(dst, return_size) {
            VMResult::Success(()) => {}
            VMResult::StackOverflow => unreachable!("validated block return must fit in stack"),
            other => unreachable!("unexpected block return copy failure: {other:?}"),
        }
        self.top = dst + return_size;
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
        let (restored, return_pc) = stack.function_return(&reference, 4, &runtime);
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
            .function_return_call(&callee, 8, 8, frame(CachedMemoryKind::Local, 3))
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

        stack.block_return(&reference, 4, 4);

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
    fn block_return_keeps_in_place_result_without_copying() {
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
        stack.push_u32(0x5555_aaaa).unwrap();
        assert_eq!(frame_top + 4, stack.top);

        stack.block_return(&reference, 0, 4);

        assert_eq!(stack.top, frame_top + 4);
        assert_eq!(
            trusted_read_u32(&stack.memory[frame_top..frame_top + 4]),
            0x5555_aaaa
        );
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
}
