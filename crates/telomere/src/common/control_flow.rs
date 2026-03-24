use std::sync::Arc;

use crate::runtime::vm;

use super::{Op, ReturnShape, StackMapSafepointKind};

#[derive(Debug, Clone)]
pub(crate) struct ControlFlowMetadataSite {
    pub instruction_ordinal: u32,
    pub kind: ControlFlowMetadataKind,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) enum ControlFlowMetadataKind {
    Jump {
        jump_operand_slots: Arc<[u8]>,
        target_ordinals: Arc<[u32]>,
    },
    Loop {
        dst_from_local_top: u32,
        param_size: u32,
        shape: ReturnShape,
    },
    BlockReturn {
        dst_from_local_top: u32,
        return_size: u32,
        shape: ReturnShape,
    },
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct StackMapSite {
    pub instruction_ordinal: u32,
    pub kind: StackMapSafepointKind,
    pub operand_bytes: u32,
    pub ref_offsets_from_operand_base: Arc<[u32]>,
}

#[derive(Debug, Clone)]
pub(crate) struct StackMapSourceSite {
    pub raw_start: usize,
    pub kind: StackMapSafepointKind,
    pub operand_bytes: u32,
    pub ref_offsets_from_operand_base: Arc<[u32]>,
}

#[derive(Debug, Clone)]
pub(crate) struct UnwindSiteMetadata {
    pub instruction_ordinal: u32,
    pub kind: StackMapSafepointKind,
    pub result_slot_from_local_top: Option<u32>,
}

#[derive(Debug, Clone)]
pub(crate) struct UnwindSourceSite {
    pub raw_start: usize,
    pub kind: StackMapSafepointKind,
    pub result_slot_from_local_top: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct SafepointMetadataCache {
    pub stack_map_site_addr: usize,
    pub unwind_site_addr: usize,
}

impl SafepointMetadataCache {
    pub(crate) const EMPTY: Self = Self {
        stack_map_site_addr: 0,
        unwind_site_addr: 0,
    };

    #[inline(always)]
    pub(crate) const fn new(stack_map_site_addr: usize, unwind_site_addr: usize) -> Self {
        Self {
            stack_map_site_addr,
            unwind_site_addr,
        }
    }

    #[inline(always)]
    pub(crate) const fn is_empty(self) -> bool {
        self.stack_map_site_addr == 0 && self.unwind_site_addr == 0
    }

    #[cfg(any(test, debug_assertions))]
    #[inline(always)]
    pub(crate) const fn stack_map_site_ptr(self) -> Option<*const StackMapSite> {
        if self.stack_map_site_addr != 0 {
            Some(self.stack_map_site_addr as *const _)
        } else {
            None
        }
    }

    #[inline(always)]
    pub(crate) const fn unwind_site_ptr(self) -> Option<*const UnwindSiteMetadata> {
        if self.unwind_site_addr != 0 {
            Some(self.unwind_site_addr as *const _)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StructuredJumpRewriteKind {
    Single { jump_slot: u8 },
    BrTable,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct StructuredJumpRewrite {
    pub(crate) ptr_op: Op,
    pub(crate) kind: StructuredJumpRewriteKind,
}

pub(crate) fn structured_jump_rewrite(op: Op) -> Option<StructuredJumpRewrite> {
    Some(if std::ptr::fn_addr_eq(op, vm::op_br as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_br_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 1 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 1 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_br_if_r0 as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_br_if_ptr_r0,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 1 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_br_if_r1 as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_br_if_ptr_r1,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 1 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_br_if_r2 as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_br_if_ptr_r2,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 1 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_br_if_r3 as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_br_if_ptr_r3,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 1 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 1 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_else as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_else_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 1 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_br_table as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_br_table_ptr,
            kind: StructuredJumpRewriteKind::BrTable,
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_local_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i32_local_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 2 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_local_eqz_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i32_local_eqz_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 2 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_local_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i32_local_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 2 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_local_eqz_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i32_local_eqz_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 2 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_local_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i64_local_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 2 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_local_eqz_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i64_local_eqz_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 2 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_local_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i64_local_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 2 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_local_eqz_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i64_local_eqz_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 2 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_local_and_imm_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i32_local_and_imm_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 3 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_local_and_imm_eqz_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i32_local_and_imm_eqz_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 3 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_local_and_imm_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i32_local_and_imm_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 3 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_local_and_imm_eqz_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i32_local_and_imm_eqz_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 3 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_local_addr_load8_u_and_imm_eqz_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i32_local_addr_load8_u_and_imm_eqz_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 4 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_local_addr_load8_u_and_imm_eqz_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i32_local_addr_load8_u_and_imm_eqz_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 4 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_seed_tee_eqz_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i32_seed_tee_eqz_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 7 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_seed_tee_eqz_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i32_seed_tee_eqz_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 7 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_seed_tee_eqz_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i64_seed_tee_eqz_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 7 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_seed_tee_eqz_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i64_seed_tee_eqz_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 7 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_seed_tee_imm_compare_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i32_seed_tee_imm_compare_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 8 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_seed_tee_imm_compare_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i32_seed_tee_imm_compare_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 8 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_seed_tee_imm_compare_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i64_seed_tee_imm_compare_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 8 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_seed_tee_imm_compare_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i64_seed_tee_imm_compare_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 8 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_seed_imm_and_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i32_seed_imm_and_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 7 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_seed_imm_and_eqz_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i32_seed_imm_and_eqz_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 7 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_seed_imm_and_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i32_seed_imm_and_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 7 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_seed_imm_and_eqz_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i32_seed_imm_and_eqz_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 7 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_seed_imm_and_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i64_seed_imm_and_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 7 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_seed_imm_and_eqz_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i64_seed_imm_and_eqz_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 7 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_seed_imm_and_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i64_seed_imm_and_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 7 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_seed_imm_and_eqz_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i64_seed_imm_and_eqz_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 7 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_seed_local_compare_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i32_seed_local_compare_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 7 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_seed_local_compare_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i32_seed_local_compare_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 7 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_seed_const_compare_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i32_seed_const_compare_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 7 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_seed_const_compare_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i32_seed_const_compare_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 7 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_seed_local_compare_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i64_seed_local_compare_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 7 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_seed_local_compare_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i64_seed_local_compare_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 7 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_seed_const_compare_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i64_seed_const_compare_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 7 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_seed_const_compare_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i64_seed_const_compare_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 7 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_f32_seed_local_compare_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_f32_seed_local_compare_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 7 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_f32_seed_local_compare_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_f32_seed_local_compare_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 7 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_f32_seed_const_compare_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_f32_seed_const_compare_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 7 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_f32_seed_const_compare_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_f32_seed_const_compare_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 7 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_f64_seed_local_compare_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_f64_seed_local_compare_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 7 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_f64_seed_local_compare_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_f64_seed_local_compare_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 7 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_f64_seed_const_compare_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_f64_seed_const_compare_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 7 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_f64_seed_const_compare_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_f64_seed_const_compare_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 7 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_local_local_ge_u_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i32_local_local_ge_u_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 3 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_local_local_compare_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i32_local_local_compare_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 3 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i32_local_const_compare_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i32_local_const_compare_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 3 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_local_local_compare_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i64_local_local_compare_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 3 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_i64_local_const_compare_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_i64_local_const_compare_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 3 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_f32_local_local_compare_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_f32_local_local_compare_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 3 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_f32_local_const_compare_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_f32_local_const_compare_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 3 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_f64_local_local_compare_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_f64_local_local_compare_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 3 },
        }
    } else if std::ptr::fn_addr_eq(op, vm::op_f64_local_const_compare_br_if as Op) {
        StructuredJumpRewrite {
            ptr_op: vm::op_f64_local_const_compare_br_if_ptr,
            kind: StructuredJumpRewriteKind::Single { jump_slot: 3 },
        }
    } else {
        return None;
    })
}
