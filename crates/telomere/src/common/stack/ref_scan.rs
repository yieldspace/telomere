use super::*;

impl Stack {
    #[cfg(any(test, debug_assertions))]
    pub(crate) fn visit_local_ref_ranges<F>(&self, reference: &LocalReference, mut visitor: F)
    where
        F: FnMut(std::ops::Range<usize>),
    {
        let Some(layout) = reference.layout else {
            return;
        };
        let layout = unsafe { layout.as_ref() };
        for run in layout.cold().local_ref_runs.iter() {
            let start = reference.local_top + run.start_from_local_top as usize;
            visitor(start..start + run.len_bytes as usize);
        }
    }

    #[cfg(any(test, debug_assertions))]
    pub(crate) fn visit_operand_ref_ranges<F>(
        &self,
        reference: &LocalReference,
        site: &crate::common::StackMapSite,
        mut visitor: F,
    ) where
        F: FnMut(std::ops::Range<usize>),
    {
        let base = self.operand_base(reference);
        for offset in site.ref_offsets_from_operand_base.iter() {
            let start = base + *offset as usize;
            visitor(start..start + 4);
        }
    }

    #[cfg(any(test, debug_assertions))]
    pub(crate) fn visit_operand_ref_ranges_ptr<F>(
        &self,
        reference: &LocalReference,
        stack_map_site_ptr: Option<*const crate::common::StackMapSite>,
        visitor: F,
    ) where
        F: FnMut(std::ops::Range<usize>),
    {
        let Some(site) = stack_map_site_ptr else {
            return;
        };
        self.visit_operand_ref_ranges(reference, unsafe { &*site }, visitor);
    }

    #[cfg(any(test, debug_assertions))]
    pub(crate) fn visit_local_and_operand_ref_ranges<F>(
        &self,
        reference: &LocalReference,
        safepoint: crate::common::SafepointMetadataCache,
        mut visitor: F,
    ) where
        F: FnMut(std::ops::Range<usize>),
    {
        self.visit_local_ref_ranges(reference, &mut visitor);
        self.visit_operand_ref_ranges_ptr(reference, safepoint.stack_map_site_ptr(), visitor);
    }

    pub(crate) fn result_slot_from_unwind_site(
        &self,
        reference: &LocalReference,
        unwind_site_ptr: Option<*const UnwindSiteMetadata>,
    ) -> Option<usize> {
        let site = unsafe { unwind_site_ptr.map(|site| &*site)? };
        site.result_slot_from_local_top.map(|slot| match site.kind {
            crate::common::StackMapSafepointKind::Loop
            | crate::common::StackMapSafepointKind::BlockReturn => {
                self.operand_base(reference) - reference.local_top + slot as usize
            }
            _ => slot as usize,
        })
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn local_ref_ranges(
        &self,
        reference: &LocalReference,
    ) -> Vec<std::ops::Range<usize>> {
        let mut ranges = Vec::new();
        self.visit_local_ref_ranges(reference, |range| ranges.push(range));
        ranges
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn operand_ref_ranges(
        &self,
        reference: &LocalReference,
        site: &crate::common::StackMapSite,
    ) -> Vec<std::ops::Range<usize>> {
        let mut ranges = Vec::new();
        self.visit_operand_ref_ranges(reference, site, |range| ranges.push(range));
        ranges
    }
}
