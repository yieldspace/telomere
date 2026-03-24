use super::{stack::CallFrameCache, Instr, LocalReference, Stack, StoreInner};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub(crate) struct StablePc(usize);

impl StablePc {
    const RELATIVE_TAG: usize = 1;

    #[allow(dead_code)]
    pub(crate) fn from_raw(raw: usize) -> Self {
        Self(raw)
    }

    #[allow(dead_code)]
    pub(crate) fn raw(self) -> usize {
        self.0
    }

    pub(crate) fn from_stable_ptr(ptr: *const Instr) -> Self {
        let ptr = ptr as usize;
        debug_assert_eq!(ptr & Self::RELATIVE_TAG, 0);
        Self(ptr)
    }

    pub(crate) fn from_relative_index(index: usize) -> Self {
        Self((index << 1) | Self::RELATIVE_TAG)
    }

    pub(crate) fn from_raw_in_frame(
        _runtime: &StoreInner,
        stack: &Stack,
        local_reference: LocalReference,
        ptr: *const Instr,
    ) -> Self {
        Self::relative_index_for_ptr(stack, local_reference, ptr)
            .map(Self::from_relative_index)
            .unwrap_or_else(|| Self::from_stable_ptr(ptr))
    }

    pub(crate) fn from_raw_in_call_frame(frame: CallFrameCache, ptr: *const Instr) -> Self {
        Self::relative_index_for_code_range(frame.code_base, frame.code_len, ptr)
            .map(Self::from_relative_index)
            .unwrap_or_else(|| Self::from_stable_ptr(ptr))
    }

    pub(crate) fn resolve(
        self,
        _runtime: &StoreInner,
        stack: &Stack,
        local_reference: LocalReference,
    ) -> *const Instr {
        match self.relative_index() {
            Some(index) => {
                let (base, len) = Self::current_frame_code_range(stack, local_reference)
                    .expect("relative continuation must resolve against a wasm frame");
                debug_assert!(index < len);
                unsafe { base.add(index) }
            }
            None => self.0 as *const Instr,
        }
    }

    pub(crate) fn resolve_in_call_frame(self, frame: CallFrameCache) -> *const Instr {
        match self.relative_index() {
            Some(index) => {
                let (base, len) = Self::code_range(frame.code_base, frame.code_len)
                    .expect("relative continuation must resolve against a wasm frame");
                debug_assert!(index < len);
                unsafe { base.add(index) }
            }
            None => self.0 as *const Instr,
        }
    }

    pub(crate) fn relative_index(self) -> Option<usize> {
        (self.0 & Self::RELATIVE_TAG == Self::RELATIVE_TAG).then_some(self.0 >> 1)
    }

    fn current_frame_code_range(
        stack: &Stack,
        local_reference: LocalReference,
    ) -> Option<(*const Instr, usize)> {
        let frame_size = local_reference.frame_bytes as usize;
        if frame_size < std::mem::size_of::<super::stack::CallStackInfo>() {
            return None;
        }
        let code_base = stack.code_base(&local_reference);
        if code_base.is_null() {
            return None;
        }
        let code_len = stack.code_len(&local_reference) as usize;
        if code_len == 0 {
            return None;
        }
        Some((code_base, code_len))
    }

    fn code_range(code_base: *const Instr, code_len: u32) -> Option<(*const Instr, usize)> {
        if code_base.is_null() || code_len == 0 {
            return None;
        }
        Some((code_base, code_len as usize))
    }

    fn relative_index_for_ptr(
        stack: &Stack,
        local_reference: LocalReference,
        ptr: *const Instr,
    ) -> Option<usize> {
        let (base, instr_len) = Self::current_frame_code_range(stack, local_reference)?;
        Self::relative_index_for_range(base, instr_len, ptr)
    }

    fn relative_index_for_code_range(
        code_base: *const Instr,
        code_len: u32,
        ptr: *const Instr,
    ) -> Option<usize> {
        let (base, instr_len) = Self::code_range(code_base, code_len)?;
        Self::relative_index_for_range(base, instr_len, ptr)
    }

    fn relative_index_for_range(
        base: *const Instr,
        instr_len: usize,
        ptr: *const Instr,
    ) -> Option<usize> {
        let instr_size = std::mem::size_of::<Instr>();
        let base_addr = base as usize;
        let ptr_addr = ptr as usize;
        let byte_len = instr_len.checked_mul(instr_size)?;
        let end_addr = base_addr.checked_add(byte_len)?;
        if !(base_addr..end_addr).contains(&ptr_addr) {
            return None;
        }
        let delta = ptr_addr - base_addr;
        if delta % instr_size != 0 {
            return None;
        }
        Some(delta / instr_size)
    }
}
