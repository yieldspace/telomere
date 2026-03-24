use std::sync::Arc;

#[cfg(test)]
use super::StackMapSite;
use super::{
    stack::{CachedMemoryKind, CallFrameCache},
    store::InstanceId,
    FrameLayoutHeader, FuncTypeIdentity, Instr, MemArg, ObjectRef, ReturnShape,
    SafepointMetadataCache, StablePc, StoreInner, UnwindSiteMetadata,
};

#[derive(Debug, Clone, Copy)]
pub(crate) struct PrecomputedCallFrame {
    pub code_addr: ObjectRef,
    pub code_base_addr: Option<usize>,
    pub code_len: u32,
    pub function_return_site_addr: usize,
    pub instance: InstanceId,
    pub memory0_kind: CachedMemoryKind,
    pub memory0_raw: u32,
}

impl PrecomputedCallFrame {
    #[inline(always)]
    pub(crate) fn materialize(self, runtime: &StoreInner) -> CallFrameCache {
        CallFrameCache {
            code_addr: self.code_addr,
            code_base: if let Some(code_base_addr) = self.code_base_addr {
                code_base_addr as *const Instr
            } else {
                runtime
                    .get_func(self.code_addr)
                    .code_pointer()
                    .unwrap_or(std::ptr::null())
            },
            code_len: self.code_len,
            function_return_site_addr: self.function_return_site_addr,
            instance: self.instance,
            memory0_kind: self.memory0_kind,
            memory0_raw: self.memory0_raw,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ContinuationSiteHeader {
    instruction_ordinal: u32,
    pc: StablePc,
    safepoint: SafepointMetadataCache,
}

impl ContinuationSiteHeader {
    #[inline(always)]
    pub(crate) const fn new(
        instruction_ordinal: u32,
        pc: StablePc,
        safepoint: SafepointMetadataCache,
    ) -> Self {
        Self {
            instruction_ordinal,
            pc,
            safepoint,
        }
    }

    #[inline(always)]
    pub(crate) const fn instruction_ordinal(self) -> u32 {
        self.instruction_ordinal
    }

    #[inline(always)]
    pub(crate) const fn pc(self) -> StablePc {
        self.pc
    }

    #[inline(always)]
    pub(crate) const fn safepoint_cache(self) -> SafepointMetadataCache {
        self.safepoint
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ControlSiteHeader {
    instruction_ordinal: u32,
    safepoint: SafepointMetadataCache,
}

impl ControlSiteHeader {
    #[inline(always)]
    pub(crate) const fn new(instruction_ordinal: u32, safepoint: SafepointMetadataCache) -> Self {
        Self {
            instruction_ordinal,
            safepoint,
        }
    }

    #[inline(always)]
    pub(crate) const fn instruction_ordinal(self) -> u32 {
        self.instruction_ordinal
    }

    #[inline(always)]
    pub(crate) fn unwind_site_ptr(self) -> Option<*const UnwindSiteMetadata> {
        self.safepoint.unwind_site_ptr()
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PrecomputedDirectCallSite {
    header: ContinuationSiteHeader,
    pub frame: PrecomputedCallFrame,
    pub param_bytes: u32,
    pub param_shape: ReturnShape,
    pub callee_layout_addr: usize,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PrecomputedImportCallSite {
    header: ContinuationSiteHeader,
    pub funcidx: u32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PrecomputedIndirectCallSite {
    header: ContinuationSiteHeader,
    pub tableidx: u32,
    pub expected_type_identity_addr: usize,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PrecomputedWaitSite {
    header: ContinuationSiteHeader,
    pub memarg: MemArg,
    pub memidx: u32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PrecomputedLoopSite {
    header: ControlSiteHeader,
    meta: u32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PrecomputedBlockReturnSite {
    header: ControlSiteHeader,
    meta: u32,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PrecomputedFunctionReturnSite {
    header: ControlSiteHeader,
}

impl PrecomputedDirectCallSite {
    #[inline(always)]
    pub(crate) const fn new(
        instruction_ordinal: u32,
        return_pc: StablePc,
        safepoint: SafepointMetadataCache,
        frame: PrecomputedCallFrame,
        param_bytes: u32,
        param_shape: ReturnShape,
        callee_layout_addr: usize,
    ) -> Self {
        Self {
            header: ContinuationSiteHeader::new(instruction_ordinal, return_pc, safepoint),
            frame,
            param_bytes,
            param_shape,
            callee_layout_addr,
        }
    }

    #[inline(always)]
    pub(crate) const fn instruction_ordinal(self) -> u32 {
        self.header.instruction_ordinal()
    }

    #[inline(always)]
    pub(crate) const fn return_pc(self) -> StablePc {
        self.header.pc()
    }

    #[inline(always)]
    pub(crate) fn callee_layout_ptr(self) -> Option<*const FrameLayoutHeader> {
        (self.callee_layout_addr != 0).then_some(self.callee_layout_addr as *const _)
    }

    #[inline(always)]
    pub(crate) fn safepoint_cache(self) -> SafepointMetadataCache {
        self.header.safepoint_cache()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DerivedRuntimeMetadata {
    pub direct_call_sites: Arc<[PrecomputedDirectCallSite]>,
    pub import_call_sites: Arc<[PrecomputedImportCallSite]>,
    pub indirect_call_sites: Arc<[PrecomputedIndirectCallSite]>,
    pub wait_sites: Arc<[PrecomputedWaitSite]>,
    pub loop_sites: Arc<[PrecomputedLoopSite]>,
    pub block_return_sites: Arc<[PrecomputedBlockReturnSite]>,
    pub function_return_site: Option<Arc<PrecomputedFunctionReturnSite>>,
}

impl PrecomputedIndirectCallSite {
    #[inline(always)]
    pub(crate) const fn new(
        instruction_ordinal: u32,
        return_pc: StablePc,
        safepoint: SafepointMetadataCache,
        tableidx: u32,
        expected_type_identity_addr: usize,
    ) -> Self {
        Self {
            header: ContinuationSiteHeader::new(instruction_ordinal, return_pc, safepoint),
            tableidx,
            expected_type_identity_addr,
        }
    }

    #[inline(always)]
    pub(crate) const fn instruction_ordinal(self) -> u32 {
        self.header.instruction_ordinal()
    }

    #[inline(always)]
    pub(crate) const fn return_pc(self) -> StablePc {
        self.header.pc()
    }

    #[inline(always)]
    pub(crate) fn expected_type_identity_ptr(self) -> *const FuncTypeIdentity {
        self.expected_type_identity_addr as *const _
    }

    #[inline(always)]
    pub(crate) fn safepoint_cache(self) -> SafepointMetadataCache {
        self.header.safepoint_cache()
    }
}

impl PrecomputedImportCallSite {
    #[inline(always)]
    pub(crate) const fn new(
        instruction_ordinal: u32,
        funcidx: u32,
        return_pc: StablePc,
        safepoint: SafepointMetadataCache,
    ) -> Self {
        Self {
            header: ContinuationSiteHeader::new(instruction_ordinal, return_pc, safepoint),
            funcidx,
        }
    }

    #[inline(always)]
    pub(crate) const fn instruction_ordinal(self) -> u32 {
        self.header.instruction_ordinal()
    }

    #[inline(always)]
    pub(crate) const fn return_pc(self) -> StablePc {
        self.header.pc()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn stack_map_site_ptr(self) -> Option<*const StackMapSite> {
        self.header.safepoint_cache().stack_map_site_ptr()
    }

    #[inline(always)]
    pub(crate) fn safepoint_cache(self) -> SafepointMetadataCache {
        self.header.safepoint_cache()
    }
}

impl PrecomputedLoopSite {
    pub(crate) const fn new(
        instruction_ordinal: u32,
        param_size: u32,
        shape: ReturnShape,
        stack_map_site_addr: usize,
        unwind_site_addr: usize,
    ) -> Self {
        Self {
            header: ControlSiteHeader::new(
                instruction_ordinal,
                SafepointMetadataCache::new(stack_map_site_addr, unwind_site_addr),
            ),
            meta: ReturnShape::encode_meta(param_size, shape),
        }
    }

    #[inline(always)]
    pub(crate) const fn instruction_ordinal(self) -> u32 {
        self.header.instruction_ordinal()
    }

    #[inline(always)]
    pub(crate) const fn param_size(self) -> u32 {
        ReturnShape::size_from_meta(self.meta)
    }

    #[inline(always)]
    pub(crate) fn unwind_site_ptr(self) -> Option<*const UnwindSiteMetadata> {
        self.header.unwind_site_ptr()
    }
}

impl PrecomputedBlockReturnSite {
    pub(crate) const fn new(
        instruction_ordinal: u32,
        return_size: u32,
        shape: ReturnShape,
        stack_map_site_addr: usize,
        unwind_site_addr: usize,
    ) -> Self {
        Self {
            header: ControlSiteHeader::new(
                instruction_ordinal,
                SafepointMetadataCache::new(stack_map_site_addr, unwind_site_addr),
            ),
            meta: ReturnShape::encode_meta(return_size, shape),
        }
    }

    #[inline(always)]
    pub(crate) const fn instruction_ordinal(self) -> u32 {
        self.header.instruction_ordinal()
    }

    #[inline(always)]
    pub(crate) const fn return_size(self) -> u32 {
        ReturnShape::size_from_meta(self.meta)
    }

    #[inline(always)]
    pub(crate) fn unwind_site_ptr(self) -> Option<*const UnwindSiteMetadata> {
        self.header.unwind_site_ptr()
    }
}

impl PrecomputedFunctionReturnSite {
    #[inline(always)]
    pub(crate) const fn new(instruction_ordinal: u32, safepoint: SafepointMetadataCache) -> Self {
        Self {
            header: ControlSiteHeader::new(instruction_ordinal, safepoint),
        }
    }

    #[inline(always)]
    pub(crate) const fn instruction_ordinal(self) -> u32 {
        self.header.instruction_ordinal()
    }

    #[inline(always)]
    pub(crate) fn unwind_site_ptr(self) -> Option<*const UnwindSiteMetadata> {
        self.header.unwind_site_ptr()
    }
}

impl PrecomputedWaitSite {
    #[inline(always)]
    pub(crate) const fn new(
        instruction_ordinal: u32,
        resume_pc: StablePc,
        safepoint: SafepointMetadataCache,
        memarg: MemArg,
        memidx: u32,
    ) -> Self {
        Self {
            header: ContinuationSiteHeader::new(instruction_ordinal, resume_pc, safepoint),
            memarg,
            memidx,
        }
    }

    #[inline(always)]
    pub(crate) const fn instruction_ordinal(self) -> u32 {
        self.header.instruction_ordinal()
    }

    #[inline(always)]
    pub(crate) const fn resume_pc(self) -> StablePc {
        self.header.pc()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn stack_map_site_ptr(self) -> Option<*const StackMapSite> {
        self.header.safepoint_cache().stack_map_site_ptr()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn unwind_site_ptr(self) -> Option<*const UnwindSiteMetadata> {
        self.header.safepoint_cache().unwind_site_ptr()
    }

    #[inline(always)]
    pub(crate) fn safepoint_cache(self) -> SafepointMetadataCache {
        self.header.safepoint_cache()
    }
}
