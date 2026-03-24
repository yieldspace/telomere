use super::{
    stack::CachedMemoryKind, CallFrameCache, FunctionInstanceData, InstanceData,
    InstanceMemorySlot, Instr, LocalMemoryId, LocalMemoryObject, LocalReference, Memory,
    MemoryHandle, ModuleInstance, ObjectRef, SafepointMetadataCache, SharedMemoryId, Stack, Store,
    StoreInner, VMResult,
};
use crate::runtime::scheduler::EffectSupplier;

pub struct ExecuteContext<'a> {
    pub stack: &'a mut Stack,
    pub local_reference: LocalReference,
    pub(crate) current_frame: CallFrameCache,
    pub(crate) safepoint: SafepointMetadataCache,
    pub store: &'a Store,
    pub gc: &'a mut StoreInner,
    pub effect: EffectSupplier<'a>,
    pub cont: *const Instr,
    pub task_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SnapshotMemoryKind {
    None,
    Local,
    Shared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExecuteContextSnapshot {
    pub(crate) default_memory: SnapshotMemoryKind,
    pub(crate) caller_memory: SnapshotMemoryKind,
    pub(crate) cont_addr: usize,
    pub(crate) task_id: u32,
}

impl ExecuteContextSnapshot {
    pub(crate) fn has_default_memory(self) -> bool {
        !matches!(self.default_memory, SnapshotMemoryKind::None)
    }
}

fn snapshot_memory_kind(kind: CachedMemoryKind) -> SnapshotMemoryKind {
    match kind {
        CachedMemoryKind::None => SnapshotMemoryKind::None,
        CachedMemoryKind::Local => SnapshotMemoryKind::Local,
        CachedMemoryKind::Shared => SnapshotMemoryKind::Shared,
    }
}

impl ExecuteContext<'_> {
    pub(crate) fn snapshot(&self) -> ExecuteContextSnapshot {
        let default_memory = snapshot_memory_kind(self.current_frame.memory0_kind);
        let caller_memory = self
            .caller_frame_cache()
            .map(|frame| snapshot_memory_kind(frame.memory0_kind))
            .unwrap_or(SnapshotMemoryKind::None);
        ExecuteContextSnapshot {
            default_memory,
            caller_memory,
            cont_addr: self.cont as usize,
            task_id: self.task_id,
        }
    }

    pub fn set_local_reference(&mut self, local_reference: LocalReference) {
        self.local_reference = local_reference;
        if local_reference.has_call_stack_info() {
            self.current_frame = self.stack.frame_cache(&local_reference);
        } else {
            self.current_frame = CallFrameCache::dummy();
        }
    }

    #[inline(always)]
    pub(crate) fn set_local_reference_with_frame(
        &mut self,
        local_reference: LocalReference,
        frame: CallFrameCache,
    ) {
        self.local_reference = local_reference;
        self.current_frame = frame;
    }

    #[inline(always)]
    pub(crate) fn set_safepoint(&mut self, safepoint: SafepointMetadataCache) {
        self.safepoint = safepoint;
    }

    #[inline(always)]
    fn caller_frame_cache(&self) -> Option<CallFrameCache> {
        let caller = self.caller_local_reference()?;
        Some(self.stack.frame_cache(&caller))
    }

    pub fn func(&self) -> &FunctionInstanceData {
        self.gc.get_func(self.current_frame.code_addr)
    }

    pub fn func_by_addr(&self, addr: ObjectRef) -> &FunctionInstanceData {
        self.gc.get_func(addr)
    }

    pub(crate) fn code(&self) -> *const Instr {
        let code = self.current_frame.code_base;
        debug_assert!(!code.is_null(), "wasm frame must have a code base");
        code
    }

    pub fn module(&self) -> &ModuleInstance {
        self.gc.get_module(self.instance().module_addr)
    }

    pub fn instance_addr(&self) -> ObjectRef {
        self.gc.object_ref_for_instance(self.current_frame.instance)
    }

    pub fn instance_id(&self) -> u32 {
        self.instance().instance_id
    }

    pub fn instance(&self) -> &InstanceData {
        self.gc.instance(self.current_frame.instance)
    }

    pub fn local_reference(&self) -> LocalReference {
        self.local_reference
    }

    pub fn memory_addr(&self) -> Option<MemoryHandle> {
        self.current_frame.memory0_handle()
    }

    #[inline(always)]
    fn memory_slot_at(&self, memidx: u32) -> Option<InstanceMemorySlot> {
        self.instance().memory_slots.get(memidx as usize).copied()
    }

    #[inline(always)]
    /// Returns the cached default local-memory id without decoding a tagged handle.
    ///
    /// # Safety
    /// - The active frame must have a default memory and its cached kind must be `Local`.
    /// - Callers must only use the returned id while `self.current_frame` remains the active frame.
    pub unsafe fn default_local_memory_id_unchecked(&self) -> LocalMemoryId {
        debug_assert_eq!(self.current_frame.memory0_kind, CachedMemoryKind::Local);
        unsafe { LocalMemoryId::from_raw_unchecked(self.current_frame.memory0_raw) }
    }

    #[inline(always)]
    /// Returns the cached default shared-memory id without decoding a tagged handle.
    ///
    /// # Safety
    /// - The active frame must have a default memory and its cached kind must be `Shared`.
    /// - Callers must only use the returned id while `self.current_frame` remains the active frame.
    pub unsafe fn default_shared_memory_id_unchecked(&self) -> SharedMemoryId {
        debug_assert_eq!(self.current_frame.memory0_kind, CachedMemoryKind::Shared);
        unsafe { SharedMemoryId::from_raw_unchecked(self.current_frame.memory0_raw) }
    }

    #[inline(always)]
    /// Returns the cached caller local-memory id without decoding a tagged handle.
    ///
    /// # Safety
    /// - A caller frame must exist and its cached default memory kind must be `Local`.
    /// - Callers must only use the returned id while that caller frame remains valid.
    pub unsafe fn caller_local_memory_id_unchecked(&self) -> LocalMemoryId {
        let frame = self
            .caller_frame_cache()
            .expect("caller frame cache required for caller local memory");
        debug_assert_eq!(frame.memory0_kind, CachedMemoryKind::Local);
        unsafe { LocalMemoryId::from_raw_unchecked(frame.memory0_raw) }
    }

    #[inline(always)]
    /// Returns the cached caller shared-memory id without decoding a tagged handle.
    ///
    /// # Safety
    /// - A caller frame must exist and its cached default memory kind must be `Shared`.
    /// - Callers must only use the returned id while that caller frame remains valid.
    pub unsafe fn caller_shared_memory_id_unchecked(&self) -> SharedMemoryId {
        let frame = self
            .caller_frame_cache()
            .expect("caller frame cache required for caller shared memory");
        debug_assert_eq!(frame.memory0_kind, CachedMemoryKind::Shared);
        unsafe { SharedMemoryId::from_raw_unchecked(frame.memory0_raw) }
    }

    #[inline(always)]
    /// Returns the typed local-memory id for `memidx` without decoding a tagged handle.
    ///
    /// # Safety
    /// - `memidx` must be in-bounds for the active instance memory list.
    /// - The memory at `memidx` must be local.
    pub unsafe fn local_memory_id_at_unchecked(&self, memidx: u32) -> LocalMemoryId {
        let slot = unsafe { self.memory_slot_at(memidx).unwrap_unchecked() };
        debug_assert!(matches!(slot, InstanceMemorySlot::Local(_)));
        match slot {
            InstanceMemorySlot::Local(id) => id,
            _ => unsafe { std::hint::unreachable_unchecked() },
        }
    }

    #[inline(always)]
    /// Returns the typed shared-memory id for `memidx` without decoding a tagged handle.
    ///
    /// # Safety
    /// - `memidx` must be in-bounds for the active instance memory list.
    /// - The memory at `memidx` must be shared.
    pub unsafe fn shared_memory_id_at_unchecked(&self, memidx: u32) -> SharedMemoryId {
        let slot = unsafe { self.memory_slot_at(memidx).unwrap_unchecked() };
        debug_assert!(matches!(slot, InstanceMemorySlot::Shared(_)));
        match slot {
            InstanceMemorySlot::Shared(id) => id,
            _ => unsafe { std::hint::unreachable_unchecked() },
        }
    }

    pub fn local_memory(&mut self) -> Option<&mut LocalMemoryObject> {
        match self.current_frame.memory0_kind {
            CachedMemoryKind::Local => Some(
                self.gc
                    .local_memory_mut(unsafe { self.default_local_memory_id_unchecked() }),
            ),
            CachedMemoryKind::None | CachedMemoryKind::Shared => None,
        }
    }

    pub fn memory(&mut self) -> Option<&mut Memory> {
        self.local_memory().map(LocalMemoryObject::memory_mut)
    }

    #[inline(always)]
    pub fn memory_handle_result(&self) -> VMResult<MemoryHandle> {
        VMResult::from_option(self.current_frame.memory0_handle(), || {
            VMResult::MemoryIndexOutOfRange
        })
    }

    #[inline(always)]
    pub fn read_memory_u8_array<const N: usize>(&mut self, offset: usize) -> VMResult<[u8; N]> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc.read_u8_array::<N>(handle, offset)
    }

    #[inline(always)]
    pub fn push_memory_to_stack<const N: usize>(&mut self, offset: usize) -> VMResult<()> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc
            .push_memory_to_stack::<N>(handle, self.stack, offset)
    }

    #[inline(always)]
    pub fn read_memory_u8(&mut self, offset: usize) -> VMResult<u8> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc.read_u8_at(handle, offset)
    }

    #[inline(always)]
    pub fn read_memory_i8(&mut self, offset: usize) -> VMResult<i8> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc.read_i8_at(handle, offset)
    }

    #[inline(always)]
    pub fn read_memory_u16(&mut self, offset: usize) -> VMResult<u16> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc.read_u16_at(handle, offset)
    }

    #[inline(always)]
    pub fn read_memory_i16(&mut self, offset: usize) -> VMResult<i16> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc.read_i16_at(handle, offset)
    }

    #[inline(always)]
    pub fn read_memory_u32(&mut self, offset: usize) -> VMResult<u32> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc.read_u32_at(handle, offset)
    }

    #[inline(always)]
    pub fn read_memory_i32(&mut self, offset: usize) -> VMResult<i32> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc.read_i32_at(handle, offset)
    }

    #[inline(always)]
    pub fn read_memory_u64(&mut self, offset: usize) -> VMResult<u64> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc.read_u64_at(handle, offset)
    }

    #[inline(always)]
    pub fn read_memory_i64(&mut self, offset: usize) -> VMResult<i64> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc.read_i64_at(handle, offset)
    }

    #[inline(always)]
    pub fn read_memory_f32(&mut self, offset: usize) -> VMResult<f32> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc.read_f32_at(handle, offset)
    }

    #[inline(always)]
    pub fn read_memory_f64(&mut self, offset: usize) -> VMResult<f64> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc.read_f64_at(handle, offset)
    }

    #[inline(always)]
    pub fn write_memory_bytes(&mut self, offset: usize, bytes: &[u8]) -> VMResult<()> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc.write_bytes(handle, offset, bytes)
    }

    #[inline(always)]
    pub fn grow_memory(&mut self, page_size_delta: u32) -> VMResult<i32> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc.grow_memory(handle, page_size_delta)
    }

    #[inline(always)]
    pub fn copy_memory(&mut self, dst: u32, src: u32, len: u32) -> VMResult<()> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc.copy_memory(handle, dst, src, len)
    }

    #[inline(always)]
    pub fn fill_memory(&mut self, ptr: u32, len: u32, data: u32) -> VMResult<()> {
        let handle = vm_try!(self.memory_handle_result());
        self.gc.fill_memory(handle, ptr, len, data)
    }

    pub fn with_memory<T>(&mut self, f: impl FnOnce(&mut Memory) -> T) -> Option<T> {
        let handle = self.current_frame.memory0_handle()?;
        let addr = self.gc.object_ref_for_memory_handle(handle);
        Some(self.gc.with_memory_by_addr(addr, f))
    }

    pub fn memory_page_size(&self) -> Option<u32> {
        match self.current_frame.memory0_kind {
            CachedMemoryKind::None => None,
            CachedMemoryKind::Local => Some(
                self.gc
                    .local_memory(unsafe { self.default_local_memory_id_unchecked() })
                    .page_size(),
            ),
            CachedMemoryKind::Shared => Some(
                self.gc
                    .shared_memory(unsafe { self.default_shared_memory_id_unchecked() })
                    .page_size(),
            ),
        }
    }

    pub fn caller_local_reference(&self) -> Option<LocalReference> {
        self.local_reference
            .has_call_stack_info()
            .then(|| self.stack.previous_local_reference(&self.local_reference))
            .filter(|reference| reference.has_call_stack_info())
    }

    pub fn caller_memory_addr(&self) -> Option<MemoryHandle> {
        self.caller_frame_cache()?.memory0_handle()
    }

    pub fn caller_local_memory(&mut self) -> Option<&mut LocalMemoryObject> {
        let frame = self.caller_frame_cache()?;
        match frame.memory0_kind {
            CachedMemoryKind::Local => Some(
                self.gc
                    .local_memory_mut(unsafe { self.caller_local_memory_id_unchecked() }),
            ),
            CachedMemoryKind::None | CachedMemoryKind::Shared => None,
        }
    }

    pub fn caller_memory(&mut self) -> Option<&mut Memory> {
        self.caller_local_memory()
            .map(LocalMemoryObject::memory_mut)
    }

    pub fn with_caller_memory<T>(&mut self, f: impl FnOnce(&mut Memory) -> T) -> Option<T> {
        let handle = self.caller_memory_addr()?;
        let addr = self.gc.object_ref_for_memory_handle(handle);
        Some(self.gc.with_memory_by_addr(addr, f))
    }

    pub fn return_slot(&mut self) -> super::ReturnSlot {
        let local_ref = self.local_reference();
        super::ReturnSlot(unsafe { self.stack.local_area_mut_ptr(&local_ref) })
    }
}
