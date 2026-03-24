use std::{ops::Deref, sync::Arc};

use super::{stack::CallStackInfo, ReturnShape, StackMapSite, UnwindSiteMetadata, ValType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LocalSlotLayout {
    pub wasm_local_index: u32,
    pub val_type: ValType,
    pub offset_from_local_top: u32,
    pub size: u32,
    pub is_ref: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RefSlotRun {
    pub start_from_local_top: u32,
    pub len_bytes: u32,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct FrameLayoutColdMetadata {
    pub local_slots: Arc<[LocalSlotLayout]>,
    pub local_ref_runs: Arc<[RefSlotRun]>,
    pub stack_map_sites: Arc<[StackMapSite]>,
    pub unwind_sites: Arc<[UnwindSiteMetadata]>,
    pub instruction_ordinal_by_raw_start: Arc<[u32]>,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub(crate) struct FrameLayoutHeader {
    pub param_bytes: u32,
    pub locals_bytes: u32,
    pub fixed_frame_bytes: u32,
    pub locals_zero_start_from_local_top: u32,
    pub footer_from_local_top: u32,
    pub operand_base_from_local_top: u32,
    pub max_operand_bytes: u32,
    pub param_shape: ReturnShape,
    pub result_shape: ReturnShape,
    cold_addr: usize,
}

impl FrameLayoutHeader {
    #[inline(always)]
    pub(crate) fn cold(&self) -> &FrameLayoutColdMetadata {
        debug_assert_ne!(self.cold_addr, 0);
        unsafe { &*(self.cold_addr as *const FrameLayoutColdMetadata) }
    }

    #[allow(dead_code)]
    pub(crate) fn stack_map_site(&self, instruction_ordinal: u32) -> Option<&StackMapSite> {
        self.cold()
            .stack_map_sites
            .iter()
            .find(|site| site.instruction_ordinal == instruction_ordinal)
    }

    #[allow(dead_code)]
    pub(crate) fn unwind_site(&self, instruction_ordinal: u32) -> Option<&UnwindSiteMetadata> {
        self.cold()
            .unwind_sites
            .iter()
            .find(|site| site.instruction_ordinal == instruction_ordinal)
    }

    #[inline(always)]
    pub(crate) fn instruction_ordinal_for_raw_start(&self, raw_start: usize) -> Option<u32> {
        self.cold()
            .instruction_ordinal_by_raw_start
            .get(raw_start)
            .copied()
            .filter(|ordinal| *ordinal != u32::MAX)
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct FrameLayoutMetadata {
    pub header: FrameLayoutHeader,
    pub cold: Arc<FrameLayoutColdMetadata>,
}

impl FrameLayoutMetadata {
    pub(crate) fn new(
        param_bytes: u32,
        locals_bytes: u32,
        max_operand_bytes: u32,
        param_shape: ReturnShape,
        result_shape: ReturnShape,
        cold: FrameLayoutColdMetadata,
    ) -> Self {
        let cold = Arc::new(cold);
        let footer_from_local_top = param_bytes + locals_bytes;
        let operand_base_from_local_top =
            footer_from_local_top + std::mem::size_of::<CallStackInfo>() as u32;
        Self {
            header: FrameLayoutHeader {
                param_bytes,
                locals_bytes,
                fixed_frame_bytes: operand_base_from_local_top,
                locals_zero_start_from_local_top: param_bytes,
                footer_from_local_top,
                operand_base_from_local_top,
                max_operand_bytes,
                param_shape,
                result_shape,
                cold_addr: Arc::as_ptr(&cold) as usize,
            },
            cold,
        }
    }

    #[inline(always)]
    pub(crate) fn header(&self) -> &FrameLayoutHeader {
        &self.header
    }

    #[allow(dead_code)]
    pub(crate) fn stack_map_site(&self, instruction_ordinal: u32) -> Option<&StackMapSite> {
        self.header.stack_map_site(instruction_ordinal)
    }

    #[allow(dead_code)]
    pub(crate) fn unwind_site(&self, instruction_ordinal: u32) -> Option<&UnwindSiteMetadata> {
        self.header.unwind_site(instruction_ordinal)
    }
}

impl Deref for FrameLayoutMetadata {
    type Target = FrameLayoutHeader;

    fn deref(&self) -> &Self::Target {
        &self.header
    }
}
