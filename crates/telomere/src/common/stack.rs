#![allow(private_interfaces)]
#[cfg(feature = "simd")]
use wide::{f32x4, f64x2, i16x8, i32x4, i64x2, i8x16, u16x8, u32x4, u64x2, u8x16};

use crate::VMResult;
use std::fmt::Debug;
use vstd::prelude::*;

use super::{
    gc::GcRef,
    memory::{
        trusted_copy_from_slice, trusted_read_u128, trusted_read_u32, trusted_read_u64,
        trusted_write_u128, trusted_write_u32, trusted_write_u64,
    },
    store::{
        FunctionInstanceData, InstanceId, InstanceMemorySlot, LocalMemoryId, MemoryHandle,
        SharedMemoryId,
    },
    Instr, StablePc, StoreInner,
};

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

/// Packed frame trailer stored at the end of each active call frame.
///
/// TCB reason: the unified stack treats this struct as a raw byte record, so its
/// layout and byte round-trip are part of the trusted stack frontier.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub(crate) struct CallStackInfo {
    return_pc: StablePc,
    prev_local_reference_top: usize,
    prev_local_reference_size: u32,
    code_addr: GcRef,
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
    pub(crate) code_addr: GcRef,
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
            code_addr: GcRef(0),
            code_base: std::ptr::null(),
            instance: InstanceId::from_index(0),
            memory0_kind: CachedMemoryKind::None,
            memory0_raw: 0,
        }
    }

    pub(crate) fn from_parts(
        code_addr: GcRef,
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

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn projection_memory(self) -> MemoryHandleProjection {
        MemoryHandleProjection::from_handle(self.memory0_handle())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn subset_matches(self, other: Self) -> bool {
        self.code_addr == other.code_addr
            && self.code_base == other.code_base
            && self.instance == other.instance
            && self.memory0_kind == other.memory0_kind
            && self.memory0_raw == other.memory0_raw
    }
}

#[inline(always)]
pub(crate) fn local_reference_has_call_stack_info(reference: LocalReference) -> bool {
    reference.local_size as usize >= std::mem::size_of::<CallStackInfo>()
}

pub trait IntoCallFrameCache {
    fn into_call_frame_cache(self, runtime: &StoreInner) -> CallFrameCache;
}

impl IntoCallFrameCache for CallFrameCache {
    fn into_call_frame_cache(self, _runtime: &StoreInner) -> CallFrameCache {
        self
    }
}

impl IntoCallFrameCache for GcRef {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
pub struct LocalReference {
    pub local_top: usize,
    pub local_size: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct MemoryHandleProjection {
    pub(crate) present: bool,
    pub(crate) shared: bool,
    pub(crate) raw: u32,
}

impl MemoryHandleProjection {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn from_handle(handle: Option<MemoryHandle>) -> Self {
        match handle {
            Some(MemoryHandle::Local(id)) => Self {
                present: true,
                shared: false,
                raw: id.raw(),
            },
            Some(MemoryHandle::Shared(id)) => Self {
                present: true,
                shared: true,
                raw: id.raw(),
            },
            None => Self {
                present: false,
                shared: false,
                raw: 0,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct FrameProjection {
    pub(crate) local_ref: LocalReference,
    pub(crate) return_pc: usize,
    pub(crate) instance_raw: u32,
    pub(crate) default_memory: MemoryHandleProjection,
    pub(crate) prev_local: LocalReference,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct StackProjection {
    pub(crate) bytes: Vec<u8>,
    pub(crate) top: usize,
    pub(crate) frame_stack: Vec<FrameProjection>,
    pub(crate) active_local: LocalReference,
}

verus! {

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct FrameProjectionParts {
    pub(crate) return_pc: usize,
    pub(crate) instance_raw: u32,
    pub(crate) default_memory_present: bool,
    pub(crate) default_memory_shared: bool,
    pub(crate) default_memory_raw: u32,
    pub(crate) prev_local_top: usize,
    pub(crate) prev_local_size: u32,
}

#[derive(Debug, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct StackProjectionParts {
    pub(crate) bytes: Vec<u8>,
    pub(crate) top: usize,
    pub(crate) frame_stack: Vec<FrameProjectionParts>,
    pub(crate) active_local_top: usize,
    pub(crate) active_local_size: u32,
}

pub(crate) open spec fn frame_view_from_projection_parts(
    parts: FrameProjectionParts,
) -> crate::common::formal::FrameView {
    crate::common::formal::frame_view_from_projection_parts(
        parts.return_pc as nat,
        parts.instance_raw,
        parts.default_memory_present,
        parts.default_memory_shared,
        parts.default_memory_raw,
        parts.prev_local_top as nat,
        parts.prev_local_size as nat,
    )
}

pub(crate) open spec fn stack_view_from_projection_parts(
    parts: StackProjectionParts,
) -> crate::common::formal::StackView {
    crate::common::formal::StackView {
        bytes: parts.bytes@,
        top: parts.top as nat,
        frame_stack: Seq::new(parts.frame_stack@.len(), |i: int| {
            frame_view_from_projection_parts(parts.frame_stack@[i])
        }),
        active_local: crate::common::formal::LocalRefView {
            local_top: parts.active_local_top as nat,
            local_size: parts.active_local_size as nat,
        },
    }
}

} // verus!

#[inline(always)]
fn local_reference_within_stack(reference: LocalReference, stack_top: usize) -> bool {
    reference
        .local_top
        .checked_add(reference.local_size as usize)
        .is_some_and(|end| end <= stack_top)
}

impl FrameProjection {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn proof_ready(&self, stack_top: usize) -> bool {
        local_reference_has_call_stack_info(self.local_ref)
            && local_reference_within_stack(self.local_ref, stack_top)
            && (self.prev_local.local_size == 0
                || local_reference_within_stack(self.prev_local, stack_top))
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn formal_builder_parts(&self) -> FrameProjectionParts {
        FrameProjectionParts {
            return_pc: self.return_pc,
            instance_raw: self.instance_raw,
            default_memory_present: self.default_memory.present,
            default_memory_shared: self.default_memory.shared,
            default_memory_raw: self.default_memory.raw,
            prev_local_top: self.prev_local.local_top,
            prev_local_size: self.prev_local.local_size,
        }
    }
}

impl StackProjection {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn proof_ready(&self) -> bool {
        if self.top > self.bytes.len() {
            return false;
        }
        if self.frame_stack.is_empty() {
            return self.active_local.local_size == 0;
        }
        if self
            .frame_stack
            .iter()
            .any(|frame| !frame.proof_ready(self.top))
        {
            return false;
        }
        if self.frame_stack.last().map(|frame| frame.local_ref) != Some(self.active_local) {
            return false;
        }
        let root_prev = self.frame_stack[0].prev_local;
        if root_prev.local_size != 0 || root_prev.local_top != 0 {
            return false;
        }
        self.frame_stack
            .windows(2)
            .all(|frames| frames[1].prev_local == frames[0].local_ref)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn formal_builder_parts(&self) -> StackProjectionParts {
        StackProjectionParts {
            bytes: self.bytes.clone(),
            top: self.top,
            frame_stack: self
                .frame_stack
                .iter()
                .map(FrameProjection::formal_builder_parts)
                .collect(),
            active_local_top: self.active_local.local_top,
            active_local_size: self.active_local.local_size,
        }
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn call_stack_metadata_len() -> usize {
    std::mem::size_of::<CallStackInfo>()
}

impl Stack {
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
    pub fn pop_u32(&mut self) -> u32 {
        self.sub_top(4);
        trusted_read_u32(&self.memory[self.top..self.top + 4])
    }
    pub fn push_u64(&mut self, v: u64) -> VMResult<()> {
        trusted_write_u64(vm_try!(self.get_memory(8)), v);
        self.add_top(8)
    }

    pub fn push_u128(&mut self, v: u128) -> VMResult<()> {
        trusted_write_u128(vm_try!(self.get_memory(16)), v);
        self.add_top(16)
    }
    pub fn pop_u128(&mut self) -> u128 {
        self.sub_top(16);
        trusted_read_u128(&self.memory[self.top..self.top + 16])
    }
    pub fn pop_u64(&mut self) -> u64 {
        self.sub_top(8);
        trusted_read_u64(&self.memory[self.top..self.top + 8])
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
    /// Returns a raw pointer to the current frame's local area.
    ///
    /// TCB reason: this is the explicit escape hatch that exposes unified-stack
    /// bytes to code outside the verified stack transition helpers.
    ///
    /// # Safety
    /// - `reference` must identify a live frame in this stack.
    /// - The returned pointer must not be used after the stack is dropped,
    ///   reallocated, or otherwise moved.
    /// - The caller must preserve frame boundaries and avoid aliasing mutable
    ///   access that would invalidate stack invariants.
    pub unsafe fn local_area_mut_ptr(&mut self, reference: &LocalReference) -> *mut u8 {
        self.memory.as_mut_ptr().add(reference.local_top)
    }
    pub fn local_tee(&mut self, reference: &LocalReference, local_addr: usize, size: usize) {
        self.memory
            .copy_within(self.top - size..self.top, reference.local_top + local_addr);
    }
    #[inline(always)]
    pub fn local_tee4(&mut self, reference: &LocalReference, local_addr: usize) {
        let value = trusted_read_u32(&self.memory[self.top - 4..self.top]);
        let start = reference.local_top + local_addr;
        trusted_write_u32(&mut self.memory[start..start + 4], value);
    }
    #[inline(always)]
    pub fn local_tee8(&mut self, reference: &LocalReference, local_addr: usize) {
        let value = trusted_read_u64(&self.memory[self.top - 8..self.top]);
        let start = reference.local_top + local_addr;
        trusted_write_u64(&mut self.memory[start..start + 8], value);
    }
    #[inline(always)]
    pub fn local_tee16(&mut self, reference: &LocalReference, local_addr: usize) {
        let value = trusted_read_u128(&self.memory[self.top - 16..self.top]);
        let start = reference.local_top + local_addr;
        trusted_write_u128(&mut self.memory[start..start + 16], value);
    }

    /// Decodes the packed `CallStackInfo` trailer from the end of one frame.
    ///
    /// Preconditions:
    /// - `reference` must describe a frame whose trailing bytes contain a valid
    ///   `CallStackInfo` written by `push_call_stack_info`.
    ///
    /// TCB reason: this performs an unaligned raw read of the packed trailer, so
    /// proofs rely on the frame layout contract staying exact.
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

    /// Encodes one `CallStackInfo` trailer into the current frame tail.
    ///
    /// Preconditions:
    /// - The caller must have reserved enough free stack space for the trailer.
    /// - `info` must be a valid packed frame descriptor for the active call.
    ///
    /// TCB reason: this converts the packed frame descriptor into raw bytes,
    /// which is part of the trusted stack/memory marshalling boundary.
    fn push_call_stack_info(&mut self, info: CallStackInfo) -> VMResult<()> {
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
    pub fn code_addr(&self, reference: &LocalReference) -> GcRef {
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

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn frame_projection(
        &self,
        reference: &LocalReference,
        runtime: &StoreInner,
    ) -> FrameProjection {
        let info = self.call_stack_info(reference);
        let prev_local = self.previous_local_reference(reference);
        let frame = self.frame_cache(reference);
        FrameProjection {
            local_ref: *reference,
            return_pc: info.return_pc.resolve(runtime, self, prev_local) as usize,
            instance_raw: frame.instance.raw(),
            default_memory: frame.projection_memory(),
            prev_local,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn projection(
        &self,
        active_local: &LocalReference,
        runtime: &StoreInner,
    ) -> StackProjection {
        let mut frame_stack = Vec::new();
        let mut cursor = *active_local;
        let frame_bytes = std::mem::size_of::<CallStackInfo>() as u32;
        while cursor.local_size >= frame_bytes {
            frame_stack.push(self.frame_projection(&cursor, runtime));
            cursor = self.previous_local_reference(&cursor);
        }
        frame_stack.reverse();
        StackProjection {
            bytes: self.memory[0..self.top].to_vec(),
            top: self.top,
            frame_stack,
            active_local: *active_local,
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
        tracing::trace!("function_return_call: {reference:?}");
        let CallStackInfo {
            return_pc,
            prev_local_reference_top,
            prev_local_reference_size,
            ..
        } = self.call_stack_info(reference);
        self.memory
            .copy_within(self.top - param_size..self.top, reference.local_top);
        self.top = reference.local_top;
        vm_try!(self.add_top(param_size));
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
        self.memory.copy_within(
            self.top - return_size..self.top,
            reference.local_top + reference.local_size as usize + stack_top,
        );
        self.top = reference.local_top + reference.local_size as usize + stack_top + return_size;
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
            code_addr: GcRef(0),
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
        let reference_local_top = reference.local_top;

        trusted_write_u32(
            &mut stack.memory[reference_local_top..reference_local_top + 4],
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
        let tail_local_size = tail.local_size;
        let callee_local_top = callee.local_top;

        assert_eq!(tail_local_top, callee_local_top);
        assert_eq!(
            tail_local_size as usize,
            8 + 8 + std::mem::size_of::<CallStackInfo>()
        );
        assert_eq!(
            trusted_read_u32(&stack.memory[tail_local_top..tail_local_top + 4]),
            0xa1a2_a3a4
        );
        assert_eq!(
            trusted_read_u32(&stack.memory[tail_local_top + 4..tail_local_top + 8]),
            0xb1b2_b3b4
        );
        assert!(stack.memory[tail_local_top + 8..tail_local_top + 16]
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
        let reference_local_top = reference.local_top;
        let reference_local_size = reference.local_size;
        let frame_top = reference_local_top + reference_local_size as usize;
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
    fn stack_projection_tracks_frame_chain_and_memory_cache() {
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
                frame(CachedMemoryKind::Local, 7),
                empty,
                std::ptr::null(),
                &runtime,
            )
            .unwrap();
        let callee = stack
            .function_call(
                0,
                8,
                frame(CachedMemoryKind::Shared, 9),
                caller,
                std::ptr::null(),
                &runtime,
            )
            .unwrap();

        let projection = stack.projection(&callee, &runtime);
        assert_eq!(projection.top, stack.top);
        assert_eq!(projection.bytes, stack.memory[0..stack.top].to_vec());
        assert_eq!(projection.active_local, callee);
        assert!(projection.proof_ready());
        assert_eq!(projection.frame_stack.len(), 2);
        assert_eq!(projection.frame_stack[0].local_ref, caller);
        assert_eq!(projection.frame_stack[1].local_ref, callee);
        assert_eq!(
            projection.frame_stack[0].default_memory,
            MemoryHandleProjection::from_handle(Some(MemoryHandle::Local(
                LocalMemoryId::from_raw(7),
            )))
        );
        assert_eq!(
            projection.frame_stack[1].default_memory,
            MemoryHandleProjection::from_handle(Some(MemoryHandle::Shared(
                SharedMemoryId::from_raw(9),
            )))
        );
        assert_eq!(projection.frame_stack[1].prev_local, caller);

        let parts = projection.formal_builder_parts();
        let callee_local_top = callee.local_top;
        let callee_local_size = callee.local_size;
        assert_eq!(parts.top, projection.top);
        assert_eq!(parts.active_local_top, callee_local_top);
        assert_eq!(parts.active_local_size, callee_local_size);

        let mut broken = projection.clone();
        broken.frame_stack[1].prev_local = empty;
        assert!(!broken.proof_ready());
    }
}
