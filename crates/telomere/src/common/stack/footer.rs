use super::*;

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub(crate) struct CallStackInfo {
    pub(super) return_pc: StablePc,
    pub(super) prev_local_reference_top: usize,
    pub(super) prev_local_reference_frame_bytes: u32,
    pub(super) prev_local_reference_layout: Option<NonNull<FrameLayoutHeader>>,
    pub(super) code_addr: ObjectRef,
    pub(super) code_base: *const Instr,
    pub(super) code_len: u32,
    pub(super) function_return_site_addr: usize,
    pub(super) instance: InstanceId,
    pub(super) memory0_kind: CachedMemoryKind,
    pub(super) memory0_raw: u32,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub(super) struct PrevLocalReferenceFooter {
    pub(super) local_top: usize,
    pub(super) frame_bytes: u32,
    pub(super) layout: Option<NonNull<FrameLayoutHeader>>,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct CallFrameCacheFooter {
    code_addr: ObjectRef,
    code_base: *const Instr,
    code_len: u32,
    function_return_site_addr: usize,
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
    pub(crate) code_len: u32,
    pub(crate) function_return_site_addr: usize,
    pub(crate) instance: InstanceId,
    pub(crate) memory0_kind: CachedMemoryKind,
    pub(crate) memory0_raw: u32,
}

impl CachedMemoryKind {
    pub(crate) fn from_memory_handle(handle: Option<MemoryHandle>) -> (Self, u32) {
        match handle {
            Some(MemoryHandle::Local(id)) => (Self::Local, id.raw()),
            Some(MemoryHandle::Shared(id)) => (Self::Shared, id.raw()),
            None => (Self::None, 0),
        }
    }
}

impl CallFrameCache {
    #[inline(always)]
    pub(crate) fn dummy() -> Self {
        Self {
            code_addr: ObjectRef(0),
            code_base: std::ptr::null(),
            code_len: 0,
            function_return_site_addr: 0,
            instance: InstanceId::from_index(0),
            memory0_kind: CachedMemoryKind::None,
            memory0_raw: 0,
        }
    }

    #[inline(always)]
    pub(crate) fn from_parts(
        code_addr: ObjectRef,
        func: &FunctionInstanceData,
        memory0: Option<MemoryHandle>,
    ) -> Self {
        let (memory0_kind, memory0_raw) = CachedMemoryKind::from_memory_handle(memory0);
        Self {
            code_addr,
            code_base: func.code_pointer().unwrap_or(std::ptr::null()),
            code_len: func.code().map_or(0, |code| {
                u32::try_from(code.len()).expect("code length overflow")
            }),
            function_return_site_addr: func
                .wasm_metadata()
                .map_or(0, |metadata| metadata.function_return_site_addr),
            instance: func.instance,
            memory0_kind,
            memory0_raw,
        }
    }

    #[inline(always)]
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

    #[inline(always)]
    pub(crate) fn function_return_site_ptr(self) -> Option<*const PrecomputedFunctionReturnSite> {
        (self.function_return_site_addr != 0).then_some(self.function_return_site_addr as *const _)
    }
}

impl From<CallFrameCacheFooter> for CallFrameCache {
    #[inline(always)]
    fn from(value: CallFrameCacheFooter) -> Self {
        Self {
            code_addr: value.code_addr,
            code_base: value.code_base,
            code_len: value.code_len,
            function_return_site_addr: value.function_return_site_addr,
            instance: value.instance,
            memory0_kind: value.memory0_kind,
            memory0_raw: value.memory0_raw,
        }
    }
}

impl From<PrevLocalReferenceFooter> for LocalReference {
    #[inline(always)]
    fn from(value: PrevLocalReferenceFooter) -> Self {
        Self {
            local_top: value.local_top,
            frame_bytes: value.frame_bytes,
            layout: value.layout,
        }
    }
}

impl Stack {
    #[inline(always)]
    pub(super) fn frame_footer_offset(reference: &LocalReference) -> usize {
        if let Some(layout) = reference.layout {
            let layout = unsafe { layout.as_ref() };
            reference.local_top + layout.footer_from_local_top as usize
        } else {
            reference.local_top + reference.frame_bytes as usize
                - std::mem::size_of::<CallStackInfo>()
        }
    }

    #[inline(always)]
    pub(crate) fn frame_footer_ptr(&self, reference: &LocalReference) -> *const CallStackInfo {
        debug_assert!(reference.has_call_stack_info());
        unsafe {
            self.memory
                .as_ptr()
                .add(Self::frame_footer_offset(reference))
                .cast::<CallStackInfo>()
        }
    }

    #[inline(always)]
    pub(crate) fn call_stack_info_ptr(&self, reference: &LocalReference) -> *const CallStackInfo {
        self.frame_footer_ptr(reference)
    }

    #[inline(always)]
    pub(crate) fn operand_base(&self, reference: &LocalReference) -> usize {
        if let Some(layout) = reference.layout {
            let layout = unsafe { layout.as_ref() };
            reference.local_top + layout.operand_base_from_local_top as usize
        } else {
            reference.local_top + reference.frame_bytes as usize
        }
    }

    #[inline(always)]
    pub(super) fn return_pc(&self, reference: &LocalReference) -> StablePc {
        unsafe { std::ptr::read_unaligned(self.call_stack_info_ptr(reference).cast::<StablePc>()) }
    }

    #[inline(always)]
    fn prev_local_reference_ptr(
        &self,
        reference: &LocalReference,
    ) -> *const PrevLocalReferenceFooter {
        unsafe {
            self.memory
                .as_ptr()
                .add(
                    Self::frame_footer_offset(reference)
                        + std::mem::offset_of!(CallStackInfo, prev_local_reference_top),
                )
                .cast::<PrevLocalReferenceFooter>()
        }
    }

    #[inline(always)]
    pub(super) fn previous_local_reference_footer(
        &self,
        reference: &LocalReference,
    ) -> PrevLocalReferenceFooter {
        unsafe { std::ptr::read_unaligned(self.prev_local_reference_ptr(reference)) }
    }

    #[inline(always)]
    fn current_frame_cache_ptr(&self, reference: &LocalReference) -> *const CallFrameCacheFooter {
        unsafe {
            self.memory
                .as_ptr()
                .add(
                    Self::frame_footer_offset(reference)
                        + std::mem::offset_of!(CallStackInfo, code_addr),
                )
                .cast::<CallFrameCacheFooter>()
        }
    }

    #[inline(always)]
    fn current_frame_cache_footer(&self, reference: &LocalReference) -> CallFrameCacheFooter {
        unsafe { std::ptr::read_unaligned(self.current_frame_cache_ptr(reference)) }
    }

    #[inline(always)]
    pub(super) fn push_call_stack_info(&mut self, info: CallStackInfo) -> VMResult<()> {
        self.flush_cached_operands();
        let size = std::mem::size_of::<CallStackInfo>();
        let end = vm_try!(self.checked_new_top(size));
        let start = self.top;
        unsafe {
            self.memory
                .as_mut_ptr()
                .add(start)
                .cast::<CallStackInfo>()
                .write_unaligned(info);
        }
        self.top = end;
        VMResult::Success(())
    }

    #[inline(always)]
    pub(crate) fn previous_local_reference(&self, reference: &LocalReference) -> LocalReference {
        self.previous_local_reference_footer(reference).into()
    }

    #[inline(always)]
    pub fn code_addr(&self, reference: &LocalReference) -> ObjectRef {
        self.current_frame_cache_footer(reference).code_addr
    }

    #[inline(always)]
    pub fn code_base(&self, reference: &LocalReference) -> *const Instr {
        self.current_frame_cache_footer(reference).code_base
    }

    #[inline(always)]
    pub fn code_len(&self, reference: &LocalReference) -> u32 {
        self.current_frame_cache_footer(reference).code_len
    }

    #[inline(always)]
    pub(crate) fn frame_cache(&self, reference: &LocalReference) -> CallFrameCache {
        self.current_frame_cache_footer(reference).into()
    }
}
