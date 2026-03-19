#![allow(private_interfaces)]

use wide::{f32x4, f64x2, i16x8, i32x4, i64x2, i8x16, u16x8, u32x4, u64x2, u8x16};

use crate::VMResult;
use std::fmt::Debug;

use super::{
    gc::GcRef,
    store::{FunctionInstanceData, InstanceId},
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
impl_lane_type!(f32x4, f32);
impl_lane_type!(f64x2, f64);
impl_lane_type!(i8x16, i8);
impl_lane_type!(i16x8, i16);
impl_lane_type!(i32x4, i32);
impl_lane_type!(i64x2, i64);
impl_lane_type!(u8x16, u8);
impl_lane_type!(u16x8, u16);
impl_lane_type!(u32x4, u32);
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
    code_addr: GcRef,
    code_base: *const Instr,
    instance: InstanceId,
    memory0_ref: GcRef,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CallFrameCache {
    pub(crate) code_addr: GcRef,
    pub(crate) code_base: *const Instr,
    pub(crate) instance: InstanceId,
    pub(crate) memory0_ref: GcRef,
}

impl CallFrameCache {
    pub(crate) fn dummy() -> Self {
        Self {
            code_addr: GcRef(0),
            code_base: std::ptr::null(),
            instance: InstanceId::from_index(0),
            memory0_ref: GcRef(0),
        }
    }

    pub(crate) fn from_parts(
        code_addr: GcRef,
        func: &FunctionInstanceData,
        memories: &[GcRef],
    ) -> Self {
        Self {
            code_addr,
            code_base: func.code_pointer().unwrap_or(std::ptr::null()),
            instance: func.instance,
            memory0_ref: memories.first().copied().unwrap_or(GcRef(0)),
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

impl IntoCallFrameCache for GcRef {
    fn into_call_frame_cache(self, runtime: &StoreInner) -> CallFrameCache {
        let func = runtime.get_func(self);
        let instance = runtime.instance(func.instance);
        CallFrameCache::from_parts(self, func, &instance.mems)
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
    #[inline(always)]
    fn reserve_top(&mut self, n: usize) -> VMResult<*mut u8> {
        let last = vm_try!(VMResult::from_option(self.top.checked_add(n), || {
            VMResult::StackOverflow
        }));
        if last > self.memory.len() {
            return VMResult::StackOverflow;
        }
        let ptr = unsafe { self.memory.as_mut_ptr().add(self.top) };
        self.top = last;
        VMResult::Success(ptr)
    }

    #[inline(always)]
    fn local_src_ptr(
        &self,
        reference: &LocalReference,
        local_addr: usize,
        size: usize,
    ) -> *const u8 {
        debug_assert!(reference.local_top + local_addr + size <= self.memory.len());
        unsafe { self.memory.as_ptr().add(reference.local_top + local_addr) }
    }

    #[inline(always)]
    fn local_dst_ptr(
        &mut self,
        reference: &LocalReference,
        local_addr: usize,
        size: usize,
    ) -> *mut u8 {
        debug_assert!(reference.local_top + local_addr + size <= self.memory.len());
        unsafe {
            self.memory
                .as_mut_ptr()
                .add(reference.local_top + local_addr)
        }
    }
    pub fn push_u8_array<const N: usize>(&mut self, v: [u8; N]) -> VMResult<()> {
        unsafe { std::ptr::copy(v.as_ptr(), vm_try!(self.get_memory(N)).as_mut_ptr(), N) };
        self.add_top(N)
    }

    pub fn push_slice(&mut self, v: &[u8]) -> VMResult<()> {
        unsafe {
            std::ptr::copy(
                v.as_ptr(),
                vm_try!(self.get_memory(v.len())).as_mut_ptr(),
                v.len(),
            )
        };
        self.add_top(v.len())
    }
    pub fn pop_u8_array<const N: usize>(&mut self) -> [u8; N] {
        self.sub_top(N);
        let mut arr = [0u8; N];
        unsafe { std::ptr::copy(self.memory.as_ptr().add(self.top), arr.as_mut_ptr(), N) };
        arr
    }
    pub fn pop_u8_array_generic<const N: usize>(&mut self, n: usize) -> [u8; N] {
        self.sub_top(n);

        let mut arr = [0u8; N];
        unsafe { std::ptr::copy(self.memory.as_ptr().add(self.top), arr.as_mut_ptr(), N) };
        arr
    }
    pub fn drop(&mut self, n: usize) -> &[u8] {
        self.sub_top(n);

        (&self.memory[self.top..self.top + n]) as _
    }
    pub fn push_u32(&mut self, v: u32) -> VMResult<()> {
        unsafe {
            vm_try!(self.reserve_top(4))
                .cast::<u32>()
                .write_unaligned(v.to_le());
        }
        VMResult::Success(())
    }
    pub fn pop_u32(&mut self) -> u32 {
        self.sub_top(4);
        unsafe {
            u32::from_le(
                self.memory
                    .as_ptr()
                    .add(self.top)
                    .cast::<u32>()
                    .read_unaligned(),
            )
        }
    }
    pub fn push_u64(&mut self, v: u64) -> VMResult<()> {
        unsafe {
            vm_try!(self.reserve_top(8))
                .cast::<u64>()
                .write_unaligned(v.to_le());
        }
        VMResult::Success(())
    }

    pub fn push_u128(&mut self, v: u128) -> VMResult<()> {
        unsafe {
            vm_try!(self.reserve_top(16))
                .cast::<u128>()
                .write_unaligned(v.to_le());
        }
        VMResult::Success(())
    }
    pub fn pop_u128(&mut self) -> u128 {
        self.sub_top(16);
        unsafe {
            u128::from_le(
                self.memory
                    .as_ptr()
                    .add(self.top)
                    .cast::<u128>()
                    .read_unaligned(),
            )
        }
    }
    pub fn pop_u64(&mut self) -> u64 {
        self.sub_top(8);
        unsafe {
            u64::from_le(
                self.memory
                    .as_ptr()
                    .add(self.top)
                    .cast::<u64>()
                    .read_unaligned(),
            )
        }
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
        let dst = vm_try!(self.reserve_top(4));
        unsafe {
            dst.cast::<u32>().write_unaligned(
                self.local_src_ptr(reference, local_addr, 4)
                    .cast::<u32>()
                    .read_unaligned(),
            );
        }
        VMResult::Success(())
    }
    #[inline(always)]
    pub fn local_get8(&mut self, reference: &LocalReference, local_addr: usize) -> VMResult<()> {
        let dst = vm_try!(self.reserve_top(8));
        unsafe {
            dst.cast::<u64>().write_unaligned(
                self.local_src_ptr(reference, local_addr, 8)
                    .cast::<u64>()
                    .read_unaligned(),
            );
        }
        VMResult::Success(())
    }
    #[inline(always)]
    pub fn local_get16(&mut self, reference: &LocalReference, local_addr: usize) -> VMResult<()> {
        let dst = vm_try!(self.reserve_top(16));
        unsafe {
            dst.cast::<u128>().write_unaligned(
                self.local_src_ptr(reference, local_addr, 16)
                    .cast::<u128>()
                    .read_unaligned(),
            );
        }
        VMResult::Success(())
    }
    pub fn local_set(&mut self, reference: &LocalReference, local_addr: usize, size: usize) {
        self.top -= size;
        self.memory
            .copy_within(self.top..self.top + size, reference.local_top + local_addr);
    }
    #[inline(always)]
    pub fn local_set4(&mut self, reference: &LocalReference, local_addr: usize) {
        self.top -= 4;
        unsafe {
            self.local_dst_ptr(reference, local_addr, 4)
                .cast::<u32>()
                .write_unaligned(
                    self.memory
                        .as_ptr()
                        .add(self.top)
                        .cast::<u32>()
                        .read_unaligned(),
                );
        }
    }
    #[inline(always)]
    pub fn local_set8(&mut self, reference: &LocalReference, local_addr: usize) {
        self.top -= 8;
        unsafe {
            self.local_dst_ptr(reference, local_addr, 8)
                .cast::<u64>()
                .write_unaligned(
                    self.memory
                        .as_ptr()
                        .add(self.top)
                        .cast::<u64>()
                        .read_unaligned(),
                );
        }
    }
    #[inline(always)]
    pub fn local_set16(&mut self, reference: &LocalReference, local_addr: usize) {
        self.top -= 16;
        unsafe {
            self.local_dst_ptr(reference, local_addr, 16)
                .cast::<u128>()
                .write_unaligned(
                    self.memory
                        .as_ptr()
                        .add(self.top)
                        .cast::<u128>()
                        .read_unaligned(),
                );
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
        unsafe {
            self.local_dst_ptr(reference, local_addr, 4)
                .cast::<u32>()
                .write_unaligned(
                    self.memory
                        .as_ptr()
                        .add(self.top - 4)
                        .cast::<u32>()
                        .read_unaligned(),
                );
        }
    }
    #[inline(always)]
    pub fn local_tee8(&mut self, reference: &LocalReference, local_addr: usize) {
        unsafe {
            self.local_dst_ptr(reference, local_addr, 8)
                .cast::<u64>()
                .write_unaligned(
                    self.memory
                        .as_ptr()
                        .add(self.top - 8)
                        .cast::<u64>()
                        .read_unaligned(),
                );
        }
    }
    #[inline(always)]
    pub fn local_tee16(&mut self, reference: &LocalReference, local_addr: usize) {
        unsafe {
            self.local_dst_ptr(reference, local_addr, 16)
                .cast::<u128>()
                .write_unaligned(
                    self.memory
                        .as_ptr()
                        .add(self.top - 16)
                        .cast::<u128>()
                        .read_unaligned(),
                );
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
    fn push_call_stack_info(&mut self, info: CallStackInfo) -> VMResult<()> {
        let size = std::mem::size_of::<CallStackInfo>();
        let bytes = unsafe {
            std::slice::from_raw_parts((&info as *const CallStackInfo).cast::<u8>(), size)
        };
        vm_try!(self.get_memory(size)).copy_from_slice(bytes);
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
    pub(crate) fn memory0_ref(&self, reference: &LocalReference) -> GcRef {
        self.call_stack_info(reference).memory0_ref
    }
    pub(crate) fn frame_cache(&self, reference: &LocalReference) -> CallFrameCache {
        let info = self.call_stack_info(reference);
        CallFrameCache {
            code_addr: info.code_addr,
            code_base: info.code_base,
            instance: info.instance,
            memory0_ref: info.memory0_ref,
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
            memory0_ref: frame.memory0_ref,
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
            memory0_ref: frame.memory0_ref,
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
stack_operation_wide!(f32x4);
stack_operation_wide!(f64x2);

stack_operation_wide!(i8x16);
stack_operation_wide!(i16x8);
stack_operation_wide!(i32x4);
stack_operation_wide!(i64x2);
stack_operation_wide!(u8x16);
stack_operation_wide!(u16x8);
stack_operation_wide!(u32x4);
stack_operation_wide!(u64x2);
