use crate::{
    common::{
        decode_local_binop64_kind, decode_local_cmp32_kind, BlockReturn, Instr, LocalFastRhsShape,
        LoopParam, MemArg, Op, VMResult,
    },
    runtime::vm::{
        op_br, op_br_if, op_br_table, op_call, op_call_i32_crc16_update16,
        op_call_i32_list_crc_summary, op_call_import, op_call_indirect, op_call_jit_lazy, op_drop,
        op_else, op_end, op_f32_abs, op_f32_add, op_f32_const, op_f32_div, op_f32_eq, op_f32_ge,
        op_f32_gt, op_f32_le, op_f32_lt, op_f32_mul, op_f32_ne, op_f32_neg, op_f32_sqrt,
        op_f32_store_const_base_local4, op_f32_sub, op_f64_abs, op_f64_add, op_f64_const,
        op_f64_convert_i32_s, op_f64_convert_i32_u, op_f64_convert_i64_s, op_f64_convert_i64_u,
        op_f64_div, op_f64_eq, op_f64_ge, op_f64_gt, op_f64_le, op_f64_load_const_base,
        op_f64_load_local_base, op_f64_lt, op_f64_mul, op_f64_ne, op_f64_neg, op_f64_sqrt,
        op_f64_store_local_base, op_f64_sub, op_global_get16, op_global_get4, op_global_get8,
        op_global_set16, op_global_set4, op_global_set8, op_i32_add, op_i32_and, op_i32_clz,
        op_i32_const, op_i32_const_binop, op_i32_const_binop_br_if, op_i32_const_binop_set4,
        op_i32_const_binop_tee4, op_i32_const_cmp, op_i32_const_cmp_br_if, op_i32_const_cmp_set4,
        op_i32_const_cmp_tee4, op_i32_const_set4, op_i32_const_tee4, op_i32_core_state_benchmark,
        op_i32_crc16_update16, op_i32_crc16_update16_masked, op_i32_ctz, op_i32_div_s,
        op_i32_div_u, op_i32_eq, op_i32_eqz, op_i32_extend16_s, op_i32_extend8_s, op_i32_ge_s,
        op_i32_ge_u, op_i32_gt_s, op_i32_gt_u, op_i32_inc_local_base, op_i32_le_s, op_i32_le_u,
        op_i32_list_crc_pair_loop, op_i32_list_crc_summary, op_i32_load, op_i32_load16_s,
        op_i32_load16_s_local_base, op_i32_load16_s_local_base_local_get4,
        op_i32_load16_s_local_base_local_get4_i32_load16_s,
        op_i32_load16_s_local_base_local_get4_i32_load16_s_local_get4,
        op_i32_load16_s_local_base_set4, op_i32_load16_s_local_base_tee4,
        op_i32_load16_s_local_base_tee4_br_if, op_i32_load16_s_local_base_tee4_i32_eqz_br_if,
        op_i32_load16_s_local_base_tee4_local_get4, op_i32_load16_s_local_get4,
        op_i32_load16_s_local_scaled_index, op_i32_load16_s_tee4_br_if,
        op_i32_load16_s_tee4_i32_eqz_br_if, op_i32_load16_u, op_i32_load16_u_local_base,
        op_i32_load16_u_local_base_local_get4, op_i32_load16_u_local_base_local_get4_i32_load16_u,
        op_i32_load16_u_local_base_local_get4_i32_load16_u_local_get4,
        op_i32_load16_u_local_base_set4, op_i32_load16_u_local_base_tee4,
        op_i32_load16_u_local_base_tee4_br_if, op_i32_load16_u_local_base_tee4_i32_eqz_br_if,
        op_i32_load16_u_local_base_tee4_local_get4, op_i32_load16_u_local_get4,
        op_i32_load16_u_local_scaled_index, op_i32_load16_u_tee4_br_if,
        op_i32_load16_u_tee4_i32_eqz_br_if, op_i32_load16_u_update_store16_local_base_loop,
        op_i32_load8_s, op_i32_load8_s_local_base, op_i32_load8_s_local_base_local_get4,
        op_i32_load8_s_local_base_set4, op_i32_load8_s_local_base_tee4,
        op_i32_load8_s_local_base_tee4_br_if, op_i32_load8_s_local_base_tee4_i32_eqz_br_if,
        op_i32_load8_s_local_base_tee4_local_get4, op_i32_load8_s_local_get4,
        op_i32_load8_s_local_scaled_index, op_i32_load8_s_tee4_br_if,
        op_i32_load8_s_tee4_i32_eqz_br_if, op_i32_load8_u, op_i32_load8_u_local_base,
        op_i32_load8_u_local_base_local_get4, op_i32_load8_u_local_base_set4,
        op_i32_load8_u_local_base_set4_local_get4, op_i32_load8_u_local_base_tee4,
        op_i32_load8_u_local_base_tee4_br_if, op_i32_load8_u_local_base_tee4_i32_eqz_br_if,
        op_i32_load8_u_local_base_tee4_local_get4, op_i32_load8_u_local_get4,
        op_i32_load8_u_local_scaled_index, op_i32_load8_u_tee4_br_if,
        op_i32_load8_u_tee4_i32_eqz_br_if, op_i32_load_const_base, op_i32_load_indexed_local_base,
        op_i32_load_indexed_shared_local_base, op_i32_load_local_base,
        op_i32_load_local_base_local_get4,
        op_i32_load_local_base_local_get4_i32_load_tee4_cmp_br_if, op_i32_load_local_base_set4,
        op_i32_load_local_base_set4_i32_load16_s_local_base,
        op_i32_load_local_base_set4_i32_load16_s_local_base_local_eq_br_if,
        op_i32_load_local_base_set4_i32_load16_s_local_base_local_get4,
        op_i32_load_local_base_set4_i32_load16_u_local_base,
        op_i32_load_local_base_set4_i32_load16_u_local_base_local_eq_br_if,
        op_i32_load_local_base_set4_i32_load16_u_local_base_local_eq_search_loop,
        op_i32_load_local_base_set4_i32_load16_u_local_base_local_eq_search_loop_fallthrough,
        op_i32_load_local_base_set4_i32_load16_u_local_base_local_get4,
        op_i32_load_local_base_set4_i32_load8_s_local_base,
        op_i32_load_local_base_set4_i32_load8_s_local_base_local_eq_br_if,
        op_i32_load_local_base_set4_i32_load8_s_local_base_local_get4,
        op_i32_load_local_base_set4_i32_load8_u_local_base,
        op_i32_load_local_base_set4_i32_load8_u_local_base_local_eq_br_if,
        op_i32_load_local_base_set4_i32_load8_u_local_base_local_get4,
        op_i32_load_local_base_set4_i32_load8_u_local_base_local_masked_search_loop,
        op_i32_load_local_base_set4_i32_load8_u_local_base_local_masked_search_loop_fallthrough,
        op_i32_load_local_base_set4_i32_load_local_base,
        op_i32_load_local_base_set4_i32_load_local_base_local_eq_br_if,
        op_i32_load_local_base_set4_i32_load_local_base_local_get4, op_i32_load_local_base_tee4,
        op_i32_load_local_base_tee4_br_if, op_i32_load_local_base_tee4_i32_eqz_br_if,
        op_i32_load_local_base_tee4_i32_load8_u_tee4_br_if, op_i32_load_local_base_tee4_local_get4,
        op_i32_load_local_get4, op_i32_load_local_scaled_index, op_i32_load_shared_local_base,
        op_i32_load_store_local_base_relink_loop, op_i32_load_store_local_base_reverse_loop,
        op_i32_load_tee4_br_if, op_i32_load_tee4_i32_eqz_br_if, op_i32_lt_s, op_i32_lt_u,
        op_i32_mul, op_i32_ne, op_i32_or, op_i32_rem_s, op_i32_rem_u, op_i32_rotl, op_i32_rotr,
        op_i32_select_bit_step4, op_i32_select_bit_step4_run, op_i32_shl, op_i32_shr_s,
        op_i32_shr_u, op_i32_store, op_i32_store16, op_i32_store16_local_base,
        op_i32_store16_local_base_local_get4, op_i32_store16_local_scaled_index, op_i32_store8,
        op_i32_store8_local_base, op_i32_store8_local_base_local_get4,
        op_i32_store8_local_scaled_index, op_i32_store_indexed_local,
        op_i32_store_indexed_local_base, op_i32_store_indexed_local_scaled_index,
        op_i32_store_local_base, op_i32_store_local_base_local_get4,
        op_i32_store_local_scaled_index, op_i32_sub, op_i32_trunc_sat_f32_s,
        op_i32_trunc_sat_f32_u, op_i32_trunc_sat_f64_s, op_i32_trunc_sat_f64_u, op_i32_wrap_i64,
        op_i32_xor, op_i64_add, op_i64_and, op_i64_const, op_i64_div_s, op_i64_div_u, op_i64_eq,
        op_i64_eqz, op_i64_extend16_s, op_i64_extend32_s, op_i64_extend8_s, op_i64_extend_i32_s,
        op_i64_extend_i32_u, op_i64_ge_s, op_i64_ge_u, op_i64_gt_s, op_i64_gt_u, op_i64_le_s,
        op_i64_le_u, op_i64_load, op_i64_load16_s, op_i64_load16_s_local_base, op_i64_load16_u,
        op_i64_load16_u_local_base, op_i64_load32_s, op_i64_load32_s_local_base, op_i64_load32_u,
        op_i64_load32_u_local_base, op_i64_load8_s, op_i64_load8_s_local_base, op_i64_load8_u,
        op_i64_load8_u_local_base, op_i64_load_local_base, op_i64_load_local_scaled_index,
        op_i64_lt_s, op_i64_lt_u, op_i64_mul, op_i64_ne, op_i64_or, op_i64_rem_s, op_i64_rem_u,
        op_i64_rotl, op_i64_rotr, op_i64_shl, op_i64_shr_s, op_i64_shr_u, op_i64_store,
        op_i64_store16_local_base, op_i64_store32_local_base, op_i64_store8_local_base,
        op_i64_store_local_base, op_i64_sub, op_i64_xor, op_if, op_local_binop32,
        op_local_binop32_br_if, op_local_binop32_set4, op_local_binop32_tee4, op_local_binop64,
        op_local_binop64_set8, op_local_binop64_tee8, op_local_cmp32, op_local_cmp32_br_if,
        op_local_cmp32_set4, op_local_cmp32_tee4, op_local_cmp64, op_local_cmp64_br_if,
        op_local_cmp64_set4, op_local_cmp64_tee4, op_local_get16, op_local_get4,
        op_local_get4_br_if, op_local_get4_br_table, op_local_get4_i32_const_add,
        op_local_get4_i32_const_add_br_if, op_local_get4_i32_const_add_br_table,
        op_local_get4_i32_const_add_i32_const_and_i32_const_compare_br_if,
        op_local_get4_i32_const_add_set4,
        op_local_get4_i32_const_add_set4_i32_load8_u_local_base_tee4_i32_eqz_br_if,
        op_local_get4_i32_const_add_tee4, op_local_get4_i32_const_add_tee4_br_if,
        op_local_get4_i32_const_and_br_if, op_local_get4_i32_const_and_eqz_br_if,
        op_local_get4_i32_const_and_i32_const_compare_br_if,
        op_local_get4_i32_const_and_tee4_i32_const_eq_br_if, op_local_get4_i32_const_compare_br_if,
        op_local_get4_i32_eqz_br_if, op_local_get4_i32_load16_s_local_base,
        op_local_get4_i32_load16_s_local_base_local_get4_i32_load16_s,
        op_local_get4_i32_load16_u_local_base,
        op_local_get4_i32_load16_u_local_base_local_get4_i32_load16_u,
        op_local_get4_i32_load8_s_local_base, op_local_get4_i32_load8_u_local_base,
        op_local_get4_i32_load_local_base, op_local_get4_local_get4,
        op_local_get4_local_get4_compare_br_if, op_local_get4_local_get4_i32_add,
        op_local_get4_local_get4_i32_add_br_if, op_local_get4_local_get4_i32_add_set4,
        op_local_get4_local_get4_i32_add_tee4,
        op_local_get4_local_get4_i32_xor_tee4_u8_shl1_i32_load16_u,
        op_local_get4_local_get4_local_get4, op_local_get4_run, op_local_get4_run_skip,
        op_local_get4_set4, op_local_get4_set4_local_get4_i32_const_compare_br_if,
        op_local_get4_tee4, op_local_get8, op_local_set16, op_local_set4, op_local_set8,
        op_local_tee16, op_local_tee4, op_local_tee8, op_local_unary32, op_local_unary32_set4,
        op_local_unary32_tee4, op_local_unary64, op_local_unary64_set8, op_local_unary64_tee8,
        op_loop, op_mem_copy_local, op_mem_fill_local, op_return, op_return_call,
        op_return_call_import, op_return_call_indirect, op_return_call_jit_lazy,
        op_scalar_copy_local_base_run, op_select, op_select16, op_select4, op_select4_set4,
        op_select4_tee4, op_select8, special_block_return, special_function_return,
        special_function_vm_end,
    },
};

#[cfg(feature = "simd")]
use crate::runtime::vm::simd::{
    f32x4_replace_lane, f64x2_replace_lane, i16x8_replace_lane, i16x8_shl, i16x8_shr,
    i32x4_replace_lane, i32x4_shl, i32x4_shr, i64x2_replace_lane, i64x2_shl, i64x2_shr,
    i8x16_replace_lane, i8x16_shl, i8x16_shr, i8x16_shuffle, i8x16_swizzle,
    op_i8x16_extract_lane_s, op_v128_bitselect, op_v128_load, op_v128_load_indexed_local,
    op_v128_load_indexed_shared, op_v128_load_shared,
};
#[cfg(feature = "threads")]
use crate::runtime::vm::{
    op_atomic_fence, op_atomic_fence_shared, op_memory_atomic_notify,
    op_memory_atomic_notify_indexed_shared, op_memory_atomic_notify_indexed_unshared,
    op_memory_atomic_notify_shared, op_memory_atomic_notify_unshared, op_memory_atomic_wait32,
    op_memory_atomic_wait32_indexed_shared, op_memory_atomic_wait32_indexed_unshared,
    op_memory_atomic_wait32_shared, op_memory_atomic_wait32_unshared, op_memory_atomic_wait64,
    op_memory_atomic_wait64_indexed_shared, op_memory_atomic_wait64_indexed_unshared,
    op_memory_atomic_wait64_shared,
};
use crate::runtime::vm::{
    op_call_cached_u16_low7_guard, op_call_i32_crc16_update16_masked,
    op_call_i32_numeric_token_state_transition, op_data_drop, op_elem_drop, op_f32_ceil,
    op_f32_convert_i32_s, op_f32_convert_i32_u, op_f32_convert_i64_s, op_f32_convert_i64_u,
    op_f32_copysign, op_f32_demote_f64, op_f32_floor, op_f32_load, op_f32_load_const_base,
    op_f32_max, op_f32_min, op_f32_nearest, op_f32_store, op_f32_store_local_base, op_f32_trunc,
    op_f64_ceil, op_f64_copysign, op_f64_floor, op_f64_load, op_f64_max, op_f64_min,
    op_f64_nearest, op_f64_promote_f32, op_f64_store, op_f64_store_const_base_local8,
    op_f64_store_local_scaled_index, op_f64_trunc,
    op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if,
    op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_false_local_get4_br_table,
    op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_taken_const_compare_br_table,
    op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_taken_local_get4_br_table,
    op_i32_inc_local_base_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if,
    op_i32_load16_s_dot4_local_base_loop, op_i32_load16_s_mul_add_local_base_delta_loop,
    op_i32_load16_s_mul_add_local_base_loop, op_i32_load16_u_bitmix_acc_local_base_delta_loop,
    op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if,
    op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_fallthrough_local_get4,
    op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_taken_local_get4,
    op_i32_load_const_base_local_get4_i32_add_set4,
    op_i32_load_local_base_set4_i32_load8_u_local_base_local_masked_compare_br_if,
    op_i32_load_store_local_base_local_get4, op_i32_matrix_i16_crc_summary,
    op_i32_numeric_token_state_transition, op_i32_popcnt, op_i32_store_const_base_local4,
    op_i32_sum_clip_local_base_loop, op_i32_trunc_f32_s, op_i32_trunc_f32_u, op_i32_trunc_f64_s,
    op_i32_trunc_f64_u, op_i64_clz, op_i64_ctz, op_i64_load_const_base, op_i64_popcnt,
    op_i64_store16, op_i64_store32, op_i64_store8, op_i64_store_const_base_local8,
    op_i64_trunc_f32_s, op_i64_trunc_f32_u, op_i64_trunc_f64_s, op_i64_trunc_f64_u,
    op_i64_trunc_sat_f32_s, op_i64_trunc_sat_f32_u, op_i64_trunc_sat_f64_s, op_i64_trunc_sat_f64_u,
    op_local_get4_i32_inc_local_base, op_local_get4_i32_inc_local_base_i32_load8_u_local_base_set4,
    op_local_get4_i32_load8_u_local_base_set4, op_local_get4_i32_load_local_base_i32_add_set4,
    op_local_get4_i32_load_local_base_i32_add_tee4,
    op_local_get4x3_i32_add_const_binop_i32_add_set4,
    op_local_get4x3_i32_add_const_binop_i32_add_tee4,
    op_local_get4x3_i32_add_const_binop_i32_add_tee4_i32_const_store, op_mem_copy,
    op_mem_copy_indexed_local_local, op_mem_copy_indexed_local_shared,
    op_mem_copy_indexed_shared_local, op_mem_copy_indexed_shared_shared, op_mem_copy_shared,
    op_mem_fill, op_mem_fill_indexed_local, op_mem_fill_indexed_shared, op_mem_fill_shared,
    op_mem_grow, op_mem_grow_indexed_local, op_mem_grow_indexed_shared, op_mem_grow_shared,
    op_mem_init, op_mem_init_indexed_local, op_mem_init_indexed_shared, op_mem_init_shared,
    op_mem_size, op_mem_size_indexed_local, op_mem_size_indexed_shared, op_mem_size_shared,
    op_ref_func, op_ref_is_null, op_ref_null, op_table_copy, op_table_fill, op_table_get,
    op_table_grow, op_table_init, op_table_set, op_table_size, op_unreachable,
    special_start_function_call, special_start_jit_function_call,
};

pub(super) enum BaselineOp {
    I32Const {
        value: u32,
    },
    I64Const {
        value: u64,
    },
    F32Const {
        bits: u32,
    },
    F64Const {
        bits: u64,
    },
    I64ExtendI32 {
        signed: bool,
    },
    I64ExtendS {
        bits: u32,
    },
    I64Eqz,
    I64Unary {
        op: I64UnaryOp,
    },
    I64Binary {
        op: I64BinaryOp,
    },
    I64Compare {
        op: I64CompareOp,
    },
    I32WrapI64,
    I32ConstWrite4 {
        value: u32,
        local: u32,
        keep_result: bool,
    },
    I32ConstBinop {
        kind: u32,
        rhs: u32,
    },
    I32ConstBinopBrIf {
        kind: u32,
        rhs: u32,
        target: usize,
    },
    I32ConstBinopWrite4 {
        kind: u32,
        rhs: u32,
        dst: u32,
        keep_result: bool,
    },
    I32ConstCmpWrite4 {
        kind: u32,
        rhs: u32,
        dst: u32,
        keep_result: bool,
    },
    I32ConstCmpBrIf {
        kind: u32,
        rhs: u32,
        target: usize,
    },
    LocalBinop32Write4 {
        kind: u32,
        lhs: u32,
        rhs: u32,
        dst: u32,
        keep_result: bool,
    },
    LocalBinop32BrIf {
        kind: u32,
        lhs: u32,
        rhs: u32,
        target: usize,
    },
    LocalBinop64 {
        kind: u32,
        lhs: u32,
        rhs: u64,
    },
    LocalBinop64Write8 {
        kind: u32,
        lhs: u32,
        rhs: u64,
        dst: u32,
        keep_result: bool,
    },
    LocalCmp32Write4 {
        kind: u32,
        lhs: u32,
        rhs: u32,
        dst: u32,
        keep_result: bool,
    },
    LocalCmp32BrIf {
        kind: u32,
        lhs: u32,
        rhs: u32,
        target: usize,
    },
    LocalCmp64Write4 {
        kind: u32,
        lhs: u32,
        rhs: u64,
        dst: u32,
        keep_result: bool,
    },
    LocalCmp64BrIf {
        kind: u32,
        lhs: u32,
        rhs: u64,
        target: usize,
    },
    LocalUnary32Write4 {
        kind: u32,
        src: u32,
        dst: u32,
        keep_result: bool,
    },
    LocalUnary64Write8 {
        kind: u32,
        src: u32,
        dst: u32,
        keep_result: bool,
    },
    LocalGet4I32ConstAdd {
        local: u32,
        value: u32,
    },
    LocalGet4I32ConstAddWrite4 {
        src: u32,
        value: u32,
        dst: u32,
        keep_result: bool,
    },
    LocalGet4LocalGet4I32Add {
        lhs: u32,
        rhs: u32,
    },
    LocalGet4LocalGet4I32AddWrite4 {
        lhs: u32,
        rhs: u32,
        dst: u32,
        keep_result: bool,
    },
    LocalGet4 {
        local: u32,
    },
    LocalGet4Run {
        locals: [u32; 16],
        count: usize,
    },
    LocalGet4RunSkip {
        locals: [u32; 16],
        count: usize,
        skip_slots: usize,
    },
    LocalGet8 {
        local: u32,
    },
    LocalGet16 {
        local: u32,
    },
    GlobalGet4 {
        index: u32,
    },
    GlobalGetSlots {
        index: u32,
        slots: usize,
    },
    GlobalSet4 {
        index: u32,
    },
    GlobalSetSlots {
        index: u32,
        slots: usize,
    },
    Drop {
        size: u32,
    },
    LocalGet4LocalGet4 {
        first: u32,
        second: u32,
    },
    LocalGet4LocalGet4Compare {
        first: u32,
        second: u32,
        op: I32CompareOp,
    },
    LocalGet4LocalGet4ConstShrUTee4Eq {
        first: u32,
        second: u32,
        shift: u32,
        dst: u32,
    },
    LocalGet4LocalGet4LocalGet4 {
        first: u32,
        second: u32,
        third: u32,
    },
    LocalGet4LocalGet4XorTee4Load16U {
        lhs: u32,
        rhs: u32,
        dst: u32,
        memarg: MemArg,
    },
    LocalGet4Write4 {
        src: u32,
        dst: u32,
        keep_result: bool,
    },
    LocalSet4 {
        local: u32,
    },
    LocalSet8 {
        local: u32,
    },
    LocalSet16 {
        local: u32,
    },
    LocalTee4 {
        local: u32,
    },
    LocalTee8 {
        local: u32,
    },
    LocalTee16 {
        local: u32,
    },
    Select4 {
        dst: Option<u32>,
        keep_result: bool,
    },
    SelectSlots {
        slots: usize,
    },
    I32Binary {
        op: I32BinaryOp,
    },
    I32Unary {
        op: I32UnaryOp,
    },
    I32Eqz,
    I32Compare {
        op: I32CompareOp,
    },
    F32Compare {
        op: FloatCompareOp,
    },
    F32Binary {
        op: FloatBinaryOp,
    },
    F32Unary {
        op: FloatUnaryOp,
    },
    F64Compare {
        op: FloatCompareOp,
    },
    F64Binary {
        op: FloatBinaryOp,
    },
    F64Unary {
        op: FloatUnaryOp,
    },
    F32ConvertI32 {
        signed: bool,
    },
    F32ConvertI64 {
        signed: bool,
    },
    F32DemoteF64,
    F64ConvertI32 {
        signed: bool,
    },
    F64ConvertI64 {
        signed: bool,
    },
    F64PromoteF32,
    I32TruncSatFloat {
        source: FloatWidth,
        signed: bool,
    },
    I32TruncFloat {
        source: FloatWidth,
        signed: bool,
    },
    I64TruncFloat {
        source: FloatWidth,
        signed: bool,
        saturating: bool,
    },
    I32Load {
        memarg: MemArg,
        width: u32,
        signed: bool,
    },
    I32LoadLocalGet4 {
        memarg: MemArg,
        width: u32,
        signed: bool,
        local: u32,
    },
    I64Load {
        memarg: MemArg,
        width: u32,
        signed: bool,
    },
    F64LoadConstBase {
        memarg: MemArg,
    },
    F64LoadLocalBase {
        local: u32,
        delta: u32,
        memarg: MemArg,
    },
    I64LoadLocalBase {
        local: u32,
        delta: u32,
        memarg: MemArg,
        width: u32,
        signed: bool,
    },
    I32LoadConstBase {
        memarg: MemArg,
    },
    I32LoadConstBaseLocalGet4I32AddSet4 {
        memarg: MemArg,
        rhs: u32,
        dst: u32,
    },
    I32LoadStoreLocalBaseLocalGet4 {
        load_kind: u32,
        store_kind: u32,
        load_memarg: MemArg,
        store_addr_local: u32,
        store_delta: u32,
        value_local: u32,
        store_memarg: MemArg,
        skip_slots: usize,
    },
    I32LoadLocalBase {
        local: u32,
        delta: u32,
        memarg: MemArg,
        width: u32,
        signed: bool,
    },
    I32LoadLocalBaseSet4 {
        local: u32,
        delta: u32,
        memarg: MemArg,
        width: u32,
        signed: bool,
        dst: u32,
        keep_result: bool,
    },
    I32LoadLocalBaseLocalGet4 {
        local: u32,
        delta: u32,
        memarg: MemArg,
        width: u32,
        signed: bool,
        dst: Option<u32>,
        preserved: u32,
    },
    I32LoadLocalBaseSet4LocalGet4 {
        local: u32,
        delta: u32,
        memarg: MemArg,
        width: u32,
        signed: bool,
        dst: u32,
        preserved: u32,
    },
    LocalGet4I32LoadLocalBase {
        preserved: u32,
        base_local: u32,
        delta: u32,
        memarg: MemArg,
        width: u32,
        signed: bool,
    },
    LocalGet4I32IncLocalBase {
        preserved: u32,
        base_local: u32,
        store_delta: u32,
        load_delta: u32,
        load_memarg: MemArg,
        store_memarg: MemArg,
    },
    LocalGet4I32Load8ULocalBaseSet4 {
        preserved: u32,
        load_base_local: u32,
        load_delta: u32,
        load_memarg: MemArg,
        dst: u32,
    },
    LocalGet4I32IncLocalBaseI32Load8ULocalBaseSet4 {
        preserved: u32,
        inc_base_local: u32,
        inc_store_delta: u32,
        inc_load_delta: u32,
        inc_load_memarg: MemArg,
        inc_store_memarg: MemArg,
        load_base_local: u32,
        load_delta: u32,
        load_memarg: MemArg,
        dst: u32,
    },
    LocalGet4I32LoadLocalBaseI32AddWrite4 {
        rhs: u32,
        base_local: u32,
        delta: u32,
        memarg: MemArg,
        dst: u32,
        keep_result: bool,
    },
    LocalGet4x3I32AddConstBinopI32AddWrite4 {
        first: u32,
        second: u32,
        third: u32,
        kind: u32,
        rhs: u32,
        dst: u32,
        skip_slots: usize,
        keep_result: bool,
    },
    LocalGet4x3I32AddConstBinopI32AddTee4I32ConstStore {
        first: u32,
        second: u32,
        third: u32,
        kind: u32,
        rhs: u32,
        dst: u32,
        value: u32,
        memarg: MemArg,
        skip_slots: usize,
    },
    I32LoadLocalBaseSet4I32LoadLocalBase {
        first_base_local: u32,
        first_delta: u32,
        first_memarg: MemArg,
        dst: u32,
        second_delta: u32,
        second_memarg: MemArg,
        second_width: u32,
        second_signed: bool,
        preserved: Option<u32>,
    },
    I32LoadLocalBaseSet4I32LoadLocalBaseEqBrIf {
        first_base_local: u32,
        first_delta: u32,
        first_memarg: MemArg,
        dst: u32,
        second_delta: u32,
        second_memarg: MemArg,
        second_width: u32,
        second_signed: bool,
        rhs: u32,
        target: usize,
    },
    I32LoadLocalBaseSet4SearchLoop {
        node_local: u32,
        data_delta: u32,
        data_memarg: MemArg,
        data_local: u32,
        field_delta: u32,
        field_memarg: MemArg,
        field_width: u32,
        rhs_local: u32,
        rhs_mask: u32,
        compare: SearchCompare,
        next_delta: u32,
        next_memarg: MemArg,
        match_target: usize,
        miss_target: Option<usize>,
    },
    I32LoadStoreLocalBaseReverseLoop {
        prev_local: u32,
        saved_local: u32,
        cursor_local: u32,
        load_memarg: MemArg,
        store_memarg: MemArg,
    },
    I32LoadStoreLocalBaseRelinkLoop {
        cursor_local: u32,
        current_local: u32,
        prev_local: u32,
        load_memarg: MemArg,
        store_memarg: MemArg,
    },
    I32Load16UpdateStore16LocalBaseLoop {
        subtract: bool,
        ptr_local: u32,
        scalar_local: u32,
        counter_local: u32,
        load_delta: u32,
        store_delta: u32,
        load_memarg: MemArg,
        store_memarg: MemArg,
    },
    I32LoadLocalBaseLocalGet4I32Load {
        first_base_local: u32,
        first_delta: u32,
        first_memarg: MemArg,
        first_width: u32,
        first_signed: bool,
        second_addr_local: u32,
        second_memarg: MemArg,
        second_width: u32,
        second_signed: bool,
    },
    I32LoadLocalBaseLocalGet4I32LoadCmpBrIf {
        first_base_local: u32,
        first_delta: u32,
        first_memarg: MemArg,
        first_width: u32,
        first_signed: bool,
        first_dst: u32,
        second_addr_local: u32,
        second_memarg: MemArg,
        second_width: u32,
        second_signed: bool,
        second_dst: u32,
        compare: I32CompareOp,
        target: usize,
    },
    I32LoadLocalScaledIndex {
        base_local: u32,
        index_local: u32,
        scale_log2: u32,
        delta: u32,
        memarg: MemArg,
        width: u32,
        signed: bool,
    },
    I32Store {
        memarg: MemArg,
        width: u32,
    },
    I64Store {
        memarg: MemArg,
        width: u32,
    },
    I64StoreLocalBase {
        base_local: u32,
        delta: u32,
        memarg: MemArg,
        width: u32,
    },
    F64StoreLocalBase {
        base_local: u32,
        delta: u32,
        memarg: MemArg,
    },
    StoreConstBaseLocal4 {
        memarg: MemArg,
        local: u32,
    },
    StoreConstBaseLocal8 {
        memarg: MemArg,
        local: u32,
    },
    I32StoreLocalBaseLocalGet4 {
        addr_local: u32,
        delta: u32,
        value_local: u32,
        memarg: MemArg,
        width: u32,
    },
    I32StoreLocalBase {
        base_local: u32,
        delta: u32,
        memarg: MemArg,
        width: u32,
    },
    I32StoreLocalScaledIndex {
        base_local: u32,
        index_local: u32,
        scale_log2: u32,
        delta: u32,
        memarg: MemArg,
        width: u32,
    },
    I32IncLocalBase {
        base_local: u32,
        store_delta: u32,
        load_delta: u32,
        load_memarg: MemArg,
        store_memarg: MemArg,
    },
    ScalarCopyLocalBaseRun {
        width: u32,
        dst_base_local: u32,
        src_base_local: u32,
        lanes: Vec<ScalarCopyLane>,
    },
    I32LoadLocalBaseTeeLoad8UTeeBrIf {
        first_base_local: u32,
        first_delta: u32,
        first_memarg: MemArg,
        first_dst: u32,
        byte_memarg: MemArg,
        byte_dst: u32,
        target: usize,
    },
    I32LoadTee4BrIf {
        memarg: MemArg,
        width: u32,
        signed: bool,
        dst: u32,
        eqz: bool,
        target: usize,
    },
    I32LoadLocalBaseTee4BrIf {
        base_local: u32,
        delta: u32,
        memarg: MemArg,
        width: u32,
        signed: bool,
        dst: u32,
        eqz: bool,
        target: usize,
    },
    I32GuardedLoad8UpdateBrIf {
        next_src: u32,
        next_delta: u32,
        next_dst: u32,
        guard_kind: u32,
        guard_lhs: u32,
        guard_rhs: u32,
        false_target: usize,
        ptr_local: u32,
        load_delta: u32,
        memarg: MemArg,
        byte_dst: u32,
        update_src: u32,
        ptr_dst: u32,
        branch_local: u32,
        true_target: usize,
    },
    I32Load8UpdateBrIf {
        ptr_local: u32,
        load_delta: u32,
        memarg: MemArg,
        byte_dst: u32,
        next_src: u32,
        ptr_dst: u32,
        branch_local: u32,
        target: usize,
    },
    LocalAddSetLoad8EqzBrIf {
        add_src: u32,
        imm: u32,
        add_dst: u32,
        load_base: u32,
        load_delta: u32,
        memarg: MemArg,
        tee_dst: u32,
        target: usize,
    },
    MemoryCopy,
    MemoryFill,
    MemorySize {
        shared: bool,
    },
    MemoryGrow {
        shared: bool,
    },
    Branch {
        target: usize,
    },
    BrIf {
        target: usize,
    },
    LocalGet4BrIf {
        local: u32,
        target: usize,
    },
    LocalGet4I32ConstAddBrIf {
        local: u32,
        imm: u32,
        target: usize,
    },
    LocalGet4LocalGet4I32AddBrIf {
        lhs: u32,
        rhs: u32,
        target: usize,
    },
    LocalGet4I32EqzBrIf {
        local: u32,
        target: usize,
    },
    LocalGet4I32ConstCompareBrIf {
        local: u32,
        kind: u32,
        rhs: u32,
        target: usize,
    },
    LocalGet4LocalGet4CompareBrIf {
        lhs: u32,
        rhs: u32,
        kind: u32,
        target: usize,
    },
    LocalGet4I32ConstAndBrIf {
        local: u32,
        mask: u32,
        eqz: bool,
        target: usize,
    },
    LocalGet4I32ConstAndI32ConstCompareBrIf {
        local: u32,
        mask: u32,
        kind: u32,
        rhs: u32,
        target: usize,
    },
    LocalGet4I32ConstAndTee4I32ConstEqBrIf {
        local: u32,
        mask: u32,
        dst: u32,
        rhs: u32,
        target: usize,
    },
    LocalGet4Set4LocalGet4I32ConstCompareBrIf {
        copy_src: u32,
        copy_dst: u32,
        lhs: u32,
        kind: u32,
        rhs: u32,
        target: usize,
    },
    LocalGet4I32ConstAddI32ConstAndI32ConstCompareBrIf {
        local: u32,
        imm: u32,
        mask: u32,
        kind: u32,
        rhs: u32,
        target: usize,
    },
    LocalGet4I32ConstAddTee4BrIf {
        src: u32,
        imm: u32,
        dst: u32,
        target: usize,
    },
    LocalGet4BrTable {
        local: u32,
        addend: u32,
        targets: Vec<usize>,
    },
    BrTable {
        targets: Vec<usize>,
    },
    If {
        else_target: usize,
    },
    Else {
        target: usize,
    },
    Loop {
        param: LoopParam,
    },
    End {
        next_is_function_return: bool,
        next_resets_stack: bool,
    },
    BlockReturn {
        block_return: BlockReturn,
    },
    FunctionReturn {
        return_size: u32,
    },
    FunctionVmEnd,
    I32Crc16Update16 {
        data_local: u32,
        crc_local: u32,
        return_target: usize,
        masked: bool,
    },
    I32CoreStateBenchmark {
        locals: [u32; 6],
        return_target: usize,
    },
    I32NumericTokenStateTransition {
        instr_ref_local: u32,
        counts_local: u32,
        return_target: usize,
    },
    I32ListCrcPairLoop {
        frame_base_local: u32,
        res_delta: u32,
        iterations_delta: u32,
        crc_delta: u32,
        target: usize,
    },
    I32ListCrcSummary {
        res_local: u32,
        finder_idx_local: u32,
        return_target: usize,
    },
    I32SelectBitStep4 {
        step: SelectBitStep4,
    },
    I32SelectBitStep4Run {
        steps: Vec<SelectBitStep4>,
    },
    CallI32Crc16Update16 {
        masked: bool,
    },
    CallI32ListCrcSummary,
    DirectCall {
        operand_index: usize,
        continuation_index: usize,
        is_return_call: bool,
    },
    IndirectCall {
        operand_index: usize,
        continuation_index: usize,
        is_return_call: bool,
    },
    AtomicFence {
        shared: bool,
    },
    RefNull,
    RefIsNull,
    RefFunc {
        funcidx: u32,
    },
    Trap {
        result: VMResult<()>,
    },
    RuntimeStub {
        pc_index: usize,
        kind: u32,
        pop_slots: usize,
        push_slots: usize,
    },
    RuntimeContinuationStub {
        pc_index: usize,
        kind: u32,
    },
}

#[derive(Clone)]
pub(super) struct SelectBitStep4 {
    pub(super) tmp_local: u32,
    pub(super) poly: u32,
    pub(super) source_local: u32,
    pub(super) source_shift: u32,
    pub(super) prev_local: u32,
    pub(super) flags: u32,
    pub(super) dst_local: u32,
}

pub(super) struct ScalarCopyLane {
    pub(super) dst_delta: u32,
    pub(super) src_delta: u32,
    pub(super) load_memarg: MemArg,
    pub(super) store_memarg: MemArg,
}

pub(super) enum I32BinaryOp {
    Add,
    Sub,
    Mul,
    DivS,
    DivU,
    RemS,
    RemU,
    And,
    Or,
    Xor,
    Shl,
    ShrS,
    ShrU,
    Rotl,
    Rotr,
}

#[derive(Clone, Copy)]
pub(super) enum I64BinaryOp {
    Add,
    Sub,
    Mul,
    DivS,
    DivU,
    RemS,
    RemU,
    And,
    Or,
    Xor,
    Shl,
    ShrS,
    ShrU,
    Rotl,
    Rotr,
}

#[derive(Clone, Copy)]
pub(super) enum I64CompareOp {
    Eq,
    Ne,
    LtS,
    LtU,
    GtS,
    GtU,
    LeS,
    LeU,
    GeS,
    GeU,
}

pub(super) enum I32UnaryOp {
    Clz,
    Ctz,
    Popcnt,
    Extend8S,
    Extend16S,
}

pub(super) enum I32CompareOp {
    Eq,
    Ne,
    LtS,
    LtU,
    GtS,
    GtU,
    LeS,
    LeU,
    GeS,
    GeU,
}

#[derive(Clone, Copy)]
pub(super) enum FloatCompareOp {
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
}

#[derive(Clone, Copy)]
pub(super) enum FloatBinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Min,
    Max,
    Copysign,
}

#[derive(Clone, Copy)]
pub(super) enum FloatUnaryOp {
    Abs,
    Neg,
    Sqrt,
    Ceil,
    Floor,
    Trunc,
    Nearest,
}

#[derive(Clone, Copy)]
pub(super) enum FloatWidth {
    F32,
    F64,
}

#[derive(Clone, Copy)]
pub(super) enum I64UnaryOp {
    Clz,
    Ctz,
    Popcnt,
}

#[derive(Clone, Copy)]
pub(super) enum SearchCompare {
    Eq,
    Ne,
}

struct OpSpec {
    op: Op,
    decode: DecodeFn,
}

type DecodeFn = fn(&[Instr], usize) -> Result<BaselineOp, ()>;

const OP_SPECS: &[OpSpec] = &[
    OpSpec {
        op: op_i32_const as Op,
        decode: decode_i32_const,
    },
    OpSpec {
        op: op_i64_const as Op,
        decode: decode_i64_const,
    },
    OpSpec {
        op: op_f32_const as Op,
        decode: decode_f32_const,
    },
    OpSpec {
        op: op_f64_const as Op,
        decode: decode_f64_const,
    },
    OpSpec {
        op: op_f32_add as Op,
        decode: decode_f32_add,
    },
    OpSpec {
        op: op_f32_sub as Op,
        decode: decode_f32_sub,
    },
    OpSpec {
        op: op_f32_mul as Op,
        decode: decode_f32_mul,
    },
    OpSpec {
        op: op_f32_div as Op,
        decode: decode_f32_div,
    },
    OpSpec {
        op: op_f32_abs as Op,
        decode: decode_f32_abs,
    },
    OpSpec {
        op: op_f32_neg as Op,
        decode: decode_f32_neg,
    },
    OpSpec {
        op: op_f32_sqrt as Op,
        decode: decode_f32_sqrt,
    },
    OpSpec {
        op: op_f32_trunc as Op,
        decode: decode_f32_trunc,
    },
    OpSpec {
        op: op_f64_add as Op,
        decode: decode_f64_add,
    },
    OpSpec {
        op: op_f64_sub as Op,
        decode: decode_f64_sub,
    },
    OpSpec {
        op: op_f64_mul as Op,
        decode: decode_f64_mul,
    },
    OpSpec {
        op: op_f64_div as Op,
        decode: decode_f64_div,
    },
    OpSpec {
        op: op_f64_neg as Op,
        decode: decode_f64_neg,
    },
    OpSpec {
        op: op_f64_abs as Op,
        decode: decode_f64_abs,
    },
    OpSpec {
        op: op_f64_sqrt as Op,
        decode: decode_f64_sqrt,
    },
    OpSpec {
        op: op_f64_ceil as Op,
        decode: decode_f64_ceil,
    },
    OpSpec {
        op: op_f64_floor as Op,
        decode: decode_f64_floor,
    },
    OpSpec {
        op: op_f64_trunc as Op,
        decode: decode_f64_trunc,
    },
    OpSpec {
        op: op_f64_nearest as Op,
        decode: decode_f64_nearest,
    },
    OpSpec {
        op: op_f64_convert_i32_s as Op,
        decode: decode_f64_convert_i32_s,
    },
    OpSpec {
        op: op_f64_convert_i32_u as Op,
        decode: decode_f64_convert_i32_u,
    },
    OpSpec {
        op: op_f64_convert_i64_s as Op,
        decode: decode_f64_convert_i64_s,
    },
    OpSpec {
        op: op_f64_convert_i64_u as Op,
        decode: decode_f64_convert_i64_u,
    },
    OpSpec {
        op: op_i64_extend_i32_u as Op,
        decode: decode_i64_extend_i32_u,
    },
    OpSpec {
        op: op_i64_extend_i32_s as Op,
        decode: decode_i64_extend_i32_s,
    },
    OpSpec {
        op: op_i64_extend8_s as Op,
        decode: decode_i64_extend8_s,
    },
    OpSpec {
        op: op_i64_extend16_s as Op,
        decode: decode_i64_extend16_s,
    },
    OpSpec {
        op: op_i64_extend32_s as Op,
        decode: decode_i64_extend32_s,
    },
    OpSpec {
        op: op_i64_eqz as Op,
        decode: decode_i64_eqz,
    },
    OpSpec {
        op: op_i64_clz as Op,
        decode: decode_i64_clz,
    },
    OpSpec {
        op: op_i64_ctz as Op,
        decode: decode_i64_ctz,
    },
    OpSpec {
        op: op_i64_popcnt as Op,
        decode: decode_i64_popcnt,
    },
    OpSpec {
        op: op_i64_eq as Op,
        decode: decode_i64_eq,
    },
    OpSpec {
        op: op_i64_ne as Op,
        decode: decode_i64_ne,
    },
    OpSpec {
        op: op_i64_lt_s as Op,
        decode: decode_i64_lt_s,
    },
    OpSpec {
        op: op_i64_lt_u as Op,
        decode: decode_i64_lt_u,
    },
    OpSpec {
        op: op_i64_gt_s as Op,
        decode: decode_i64_gt_s,
    },
    OpSpec {
        op: op_i64_gt_u as Op,
        decode: decode_i64_gt_u,
    },
    OpSpec {
        op: op_i64_le_s as Op,
        decode: decode_i64_le_s,
    },
    OpSpec {
        op: op_i64_le_u as Op,
        decode: decode_i64_le_u,
    },
    OpSpec {
        op: op_i64_ge_s as Op,
        decode: decode_i64_ge_s,
    },
    OpSpec {
        op: op_i64_ge_u as Op,
        decode: decode_i64_ge_u,
    },
    OpSpec {
        op: op_i64_add as Op,
        decode: decode_i64_add,
    },
    OpSpec {
        op: op_i64_mul as Op,
        decode: decode_i64_mul,
    },
    OpSpec {
        op: op_i64_div_s as Op,
        decode: decode_i64_div_s,
    },
    OpSpec {
        op: op_i64_div_u as Op,
        decode: decode_i64_div_u,
    },
    OpSpec {
        op: op_i64_rem_s as Op,
        decode: decode_i64_rem_s,
    },
    OpSpec {
        op: op_i64_rem_u as Op,
        decode: decode_i64_rem_u,
    },
    OpSpec {
        op: op_i64_sub as Op,
        decode: decode_i64_sub,
    },
    OpSpec {
        op: op_i64_and as Op,
        decode: decode_i64_and,
    },
    OpSpec {
        op: op_i64_or as Op,
        decode: decode_i64_or,
    },
    OpSpec {
        op: op_i64_xor as Op,
        decode: decode_i64_xor,
    },
    OpSpec {
        op: op_i64_shl as Op,
        decode: decode_i64_shl,
    },
    OpSpec {
        op: op_i64_shr_s as Op,
        decode: decode_i64_shr_s,
    },
    OpSpec {
        op: op_i64_shr_u as Op,
        decode: decode_i64_shr_u,
    },
    OpSpec {
        op: op_i64_rotl as Op,
        decode: decode_i64_rotl,
    },
    OpSpec {
        op: op_i64_rotr as Op,
        decode: decode_i64_rotr,
    },
    OpSpec {
        op: op_i32_wrap_i64 as Op,
        decode: decode_i32_wrap_i64,
    },
    OpSpec {
        op: op_i32_const_set4 as Op,
        decode: decode_i32_const_set4,
    },
    OpSpec {
        op: op_i32_const_tee4 as Op,
        decode: decode_i32_const_tee4,
    },
    OpSpec {
        op: op_i32_const_binop as Op,
        decode: decode_i32_const_binop,
    },
    OpSpec {
        op: op_i32_const_binop_br_if as Op,
        decode: decode_i32_const_binop_br_if,
    },
    OpSpec {
        op: op_i32_const_binop_set4 as Op,
        decode: decode_i32_const_binop_set4,
    },
    OpSpec {
        op: op_i32_const_binop_tee4 as Op,
        decode: decode_i32_const_binop_tee4,
    },
    OpSpec {
        op: op_i32_const_cmp as Op,
        decode: decode_i32_const_cmp,
    },
    OpSpec {
        op: op_i32_const_cmp_br_if as Op,
        decode: decode_i32_const_cmp_br_if,
    },
    OpSpec {
        op: op_i32_const_cmp_set4 as Op,
        decode: decode_i32_const_cmp_set4,
    },
    OpSpec {
        op: op_i32_const_cmp_tee4 as Op,
        decode: decode_i32_const_cmp_tee4,
    },
    OpSpec {
        op: op_local_binop32 as Op,
        decode: decode_local_binop32,
    },
    OpSpec {
        op: op_local_binop32_br_if as Op,
        decode: decode_local_binop32_br_if,
    },
    OpSpec {
        op: op_local_binop32_set4 as Op,
        decode: decode_local_binop32_set4,
    },
    OpSpec {
        op: op_local_binop32_tee4 as Op,
        decode: decode_local_binop32_tee4,
    },
    OpSpec {
        op: op_local_binop64 as Op,
        decode: decode_local_binop64,
    },
    OpSpec {
        op: op_local_binop64_set8 as Op,
        decode: decode_local_binop64_set8,
    },
    OpSpec {
        op: op_local_binop64_tee8 as Op,
        decode: decode_local_binop64_tee8,
    },
    OpSpec {
        op: op_local_cmp32 as Op,
        decode: decode_local_cmp32,
    },
    OpSpec {
        op: op_local_cmp32_br_if as Op,
        decode: decode_local_cmp32_br_if,
    },
    OpSpec {
        op: op_local_cmp32_set4 as Op,
        decode: decode_local_cmp32_set4,
    },
    OpSpec {
        op: op_local_cmp32_tee4 as Op,
        decode: decode_local_cmp32_tee4,
    },
    OpSpec {
        op: op_local_cmp64 as Op,
        decode: decode_local_cmp64,
    },
    OpSpec {
        op: op_local_cmp64_set4 as Op,
        decode: decode_local_cmp64_set4,
    },
    OpSpec {
        op: op_local_cmp64_tee4 as Op,
        decode: decode_local_cmp64_tee4,
    },
    OpSpec {
        op: op_local_cmp64_br_if as Op,
        decode: decode_local_cmp64_br_if,
    },
    OpSpec {
        op: op_local_unary32 as Op,
        decode: decode_local_unary32,
    },
    OpSpec {
        op: op_local_unary32_set4 as Op,
        decode: decode_local_unary32_set4,
    },
    OpSpec {
        op: op_local_unary32_tee4 as Op,
        decode: decode_local_unary32_tee4,
    },
    OpSpec {
        op: op_local_unary64 as Op,
        decode: decode_local_unary64,
    },
    OpSpec {
        op: op_local_unary64_set8 as Op,
        decode: decode_local_unary64_set8,
    },
    OpSpec {
        op: op_local_unary64_tee8 as Op,
        decode: decode_local_unary64_tee8,
    },
    OpSpec {
        op: op_local_get4_i32_const_add as Op,
        decode: decode_local_get4_i32_const_add,
    },
    OpSpec {
        op: op_local_get4_i32_const_add_set4 as Op,
        decode: decode_local_get4_i32_const_add_set4,
    },
    OpSpec {
        op: op_local_get4_i32_const_add_tee4 as Op,
        decode: decode_local_get4_i32_const_add_tee4,
    },
    OpSpec {
        op: op_local_get4_local_get4_i32_add as Op,
        decode: decode_local_get4_local_get4_i32_add,
    },
    OpSpec {
        op: op_local_get4_local_get4_i32_add_set4 as Op,
        decode: decode_local_get4_local_get4_i32_add_set4,
    },
    OpSpec {
        op: op_local_get4_local_get4_i32_add_tee4 as Op,
        decode: decode_local_get4_local_get4_i32_add_tee4,
    },
    OpSpec {
        op: op_local_get4 as Op,
        decode: decode_local_get4,
    },
    OpSpec {
        op: op_local_get4_run as Op,
        decode: decode_local_get4_run,
    },
    OpSpec {
        op: op_local_get4_run_skip as Op,
        decode: decode_local_get4_run_skip,
    },
    OpSpec {
        op: op_local_get8 as Op,
        decode: decode_local_get8,
    },
    OpSpec {
        op: op_local_get16 as Op,
        decode: decode_local_get16,
    },
    OpSpec {
        op: op_global_get4 as Op,
        decode: decode_global_get4,
    },
    OpSpec {
        op: op_global_get8 as Op,
        decode: decode_global_get8,
    },
    OpSpec {
        op: op_global_get16 as Op,
        decode: decode_global_get16,
    },
    OpSpec {
        op: op_global_set4 as Op,
        decode: decode_global_set4,
    },
    OpSpec {
        op: op_global_set8 as Op,
        decode: decode_global_set8,
    },
    OpSpec {
        op: op_global_set16 as Op,
        decode: decode_global_set16,
    },
    OpSpec {
        op: op_drop as Op,
        decode: decode_drop,
    },
    OpSpec {
        op: op_local_get4_local_get4 as Op,
        decode: decode_local_get4_local_get4,
    },
    OpSpec {
        op: op_local_get4_local_get4_local_get4 as Op,
        decode: decode_local_get4_local_get4_local_get4,
    },
    OpSpec {
        op: op_local_get4_local_get4_i32_xor_tee4_u8_shl1_i32_load16_u as Op,
        decode: decode_local_get4_local_get4_xor_tee4_load16_u,
    },
    OpSpec {
        op: op_local_get4_i32_load_local_base as Op,
        decode: decode_local_get4_i32_load_local_base,
    },
    OpSpec {
        op: op_local_get4_i32_load8_u_local_base as Op,
        decode: decode_local_get4_i32_load8_u_local_base,
    },
    OpSpec {
        op: op_local_get4_i32_load8_s_local_base as Op,
        decode: decode_local_get4_i32_load8_s_local_base,
    },
    OpSpec {
        op: op_local_get4_i32_load16_u_local_base as Op,
        decode: decode_local_get4_i32_load16_u_local_base,
    },
    OpSpec {
        op: op_local_get4_i32_load16_u_local_base_local_get4_i32_load16_u as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(
                code,
                cursor,
                RUNTIME_CONT_LOCAL_GET4_I32_LOAD16_U_LOCAL_BASE_LOCAL_GET4_I32_LOAD16_U,
            )
        },
    },
    OpSpec {
        op: op_local_get4_i32_load16_s_local_base as Op,
        decode: decode_local_get4_i32_load16_s_local_base,
    },
    OpSpec {
        op: op_local_get4_i32_load16_s_local_base_local_get4_i32_load16_s as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(
                code,
                cursor,
                RUNTIME_CONT_LOCAL_GET4_I32_LOAD16_S_LOCAL_BASE_LOCAL_GET4_I32_LOAD16_S,
            )
        },
    },
    OpSpec {
        op: op_local_get4_set4 as Op,
        decode: decode_local_get4_set4,
    },
    OpSpec {
        op: op_local_get4_tee4 as Op,
        decode: decode_local_get4_tee4,
    },
    OpSpec {
        op: op_local_set4 as Op,
        decode: decode_local_set4,
    },
    OpSpec {
        op: op_local_set8 as Op,
        decode: decode_local_set8,
    },
    OpSpec {
        op: op_local_set16 as Op,
        decode: decode_local_set16,
    },
    OpSpec {
        op: op_local_tee4 as Op,
        decode: decode_local_tee4,
    },
    OpSpec {
        op: op_local_tee8 as Op,
        decode: decode_local_tee8,
    },
    OpSpec {
        op: op_local_tee16 as Op,
        decode: decode_local_tee16,
    },
    OpSpec {
        op: op_select as Op,
        decode: decode_select,
    },
    OpSpec {
        op: op_select4 as Op,
        decode: decode_select4,
    },
    OpSpec {
        op: op_select8 as Op,
        decode: decode_select8,
    },
    OpSpec {
        op: op_select16 as Op,
        decode: decode_select16,
    },
    OpSpec {
        op: op_select4_set4 as Op,
        decode: decode_select4_set4,
    },
    OpSpec {
        op: op_select4_tee4 as Op,
        decode: decode_select4_tee4,
    },
    OpSpec {
        op: op_i32_add as Op,
        decode: decode_i32_add,
    },
    OpSpec {
        op: op_i32_sub as Op,
        decode: decode_i32_sub,
    },
    OpSpec {
        op: op_i32_mul as Op,
        decode: decode_i32_mul,
    },
    OpSpec {
        op: op_i32_div_s as Op,
        decode: decode_i32_div_s,
    },
    OpSpec {
        op: op_i32_div_u as Op,
        decode: decode_i32_div_u,
    },
    OpSpec {
        op: op_i32_rem_s as Op,
        decode: decode_i32_rem_s,
    },
    OpSpec {
        op: op_i32_rem_u as Op,
        decode: decode_i32_rem_u,
    },
    OpSpec {
        op: op_i32_and as Op,
        decode: decode_i32_and,
    },
    OpSpec {
        op: op_i32_or as Op,
        decode: decode_i32_or,
    },
    OpSpec {
        op: op_i32_xor as Op,
        decode: decode_i32_xor,
    },
    OpSpec {
        op: op_i32_shl as Op,
        decode: decode_i32_shl,
    },
    OpSpec {
        op: op_i32_shr_s as Op,
        decode: decode_i32_shr_s,
    },
    OpSpec {
        op: op_i32_shr_u as Op,
        decode: decode_i32_shr_u,
    },
    OpSpec {
        op: op_i32_rotl as Op,
        decode: decode_i32_rotl,
    },
    OpSpec {
        op: op_i32_rotr as Op,
        decode: decode_i32_rotr,
    },
    OpSpec {
        op: op_i32_clz as Op,
        decode: decode_i32_clz,
    },
    OpSpec {
        op: op_i32_ctz as Op,
        decode: decode_i32_ctz,
    },
    OpSpec {
        op: op_i32_extend8_s as Op,
        decode: decode_i32_extend8_s,
    },
    OpSpec {
        op: op_i32_extend16_s as Op,
        decode: decode_i32_extend16_s,
    },
    OpSpec {
        op: op_i32_eqz as Op,
        decode: decode_i32_eqz,
    },
    OpSpec {
        op: op_i32_eq as Op,
        decode: decode_i32_eq,
    },
    OpSpec {
        op: op_i32_ne as Op,
        decode: decode_i32_ne,
    },
    OpSpec {
        op: op_i32_lt_s as Op,
        decode: decode_i32_lt_s,
    },
    OpSpec {
        op: op_i32_lt_u as Op,
        decode: decode_i32_lt_u,
    },
    OpSpec {
        op: op_i32_gt_s as Op,
        decode: decode_i32_gt_s,
    },
    OpSpec {
        op: op_i32_gt_u as Op,
        decode: decode_i32_gt_u,
    },
    OpSpec {
        op: op_i32_le_s as Op,
        decode: decode_i32_le_s,
    },
    OpSpec {
        op: op_i32_le_u as Op,
        decode: decode_i32_le_u,
    },
    OpSpec {
        op: op_i32_ge_s as Op,
        decode: decode_i32_ge_s,
    },
    OpSpec {
        op: op_i32_ge_u as Op,
        decode: decode_i32_ge_u,
    },
    OpSpec {
        op: op_f32_eq as Op,
        decode: decode_f32_eq,
    },
    OpSpec {
        op: op_f32_ne as Op,
        decode: decode_f32_ne,
    },
    OpSpec {
        op: op_f32_lt as Op,
        decode: decode_f32_lt,
    },
    OpSpec {
        op: op_f32_gt as Op,
        decode: decode_f32_gt,
    },
    OpSpec {
        op: op_f32_le as Op,
        decode: decode_f32_le,
    },
    OpSpec {
        op: op_f32_ge as Op,
        decode: decode_f32_ge,
    },
    OpSpec {
        op: op_f64_eq as Op,
        decode: decode_f64_eq,
    },
    OpSpec {
        op: op_f64_ne as Op,
        decode: decode_f64_ne,
    },
    OpSpec {
        op: op_f64_lt as Op,
        decode: decode_f64_lt,
    },
    OpSpec {
        op: op_f64_gt as Op,
        decode: decode_f64_gt,
    },
    OpSpec {
        op: op_f64_le as Op,
        decode: decode_f64_le,
    },
    OpSpec {
        op: op_f64_ge as Op,
        decode: decode_f64_ge,
    },
    OpSpec {
        op: op_i32_trunc_sat_f32_s as Op,
        decode: decode_i32_trunc_sat_f32_s,
    },
    OpSpec {
        op: op_i32_trunc_sat_f32_u as Op,
        decode: decode_i32_trunc_sat_f32_u,
    },
    OpSpec {
        op: op_i32_trunc_sat_f64_s as Op,
        decode: decode_i32_trunc_sat_f64_s,
    },
    OpSpec {
        op: op_i32_trunc_sat_f64_u as Op,
        decode: decode_i32_trunc_sat_f64_u,
    },
    OpSpec {
        op: op_i32_load as Op,
        decode: decode_i32_load,
    },
    OpSpec {
        op: op_i32_load8_u as Op,
        decode: decode_i32_load8_u,
    },
    OpSpec {
        op: op_i32_load8_s as Op,
        decode: decode_i32_load8_s,
    },
    OpSpec {
        op: op_i32_load16_u as Op,
        decode: decode_i32_load16_u,
    },
    OpSpec {
        op: op_i32_load16_s as Op,
        decode: decode_i32_load16_s,
    },
    OpSpec {
        op: op_i32_load_local_get4 as Op,
        decode: decode_i32_load_local_get4,
    },
    OpSpec {
        op: op_i32_load8_u_local_get4 as Op,
        decode: decode_i32_load8_u_local_get4,
    },
    OpSpec {
        op: op_i32_load8_s_local_get4 as Op,
        decode: decode_i32_load8_s_local_get4,
    },
    OpSpec {
        op: op_i32_load16_u_local_get4 as Op,
        decode: decode_i32_load16_u_local_get4,
    },
    OpSpec {
        op: op_i32_load16_s_local_get4 as Op,
        decode: decode_i32_load16_s_local_get4,
    },
    OpSpec {
        op: op_i64_load as Op,
        decode: decode_i64_load,
    },
    OpSpec {
        op: op_i64_load8_s as Op,
        decode: decode_i64_load8_s,
    },
    OpSpec {
        op: op_i64_load8_u as Op,
        decode: decode_i64_load8_u,
    },
    OpSpec {
        op: op_i64_load16_s as Op,
        decode: decode_i64_load16_s,
    },
    OpSpec {
        op: op_i64_load16_u as Op,
        decode: decode_i64_load16_u,
    },
    OpSpec {
        op: op_i64_load32_s as Op,
        decode: decode_i64_load32_s,
    },
    OpSpec {
        op: op_i64_load32_u as Op,
        decode: decode_i64_load32_u,
    },
    OpSpec {
        op: op_f64_load_const_base as Op,
        decode: decode_f64_load_const_base,
    },
    OpSpec {
        op: op_f64_load_local_base as Op,
        decode: decode_f64_load_local_base,
    },
    OpSpec {
        op: op_i64_load_local_base as Op,
        decode: decode_i64_load_local_base,
    },
    OpSpec {
        op: op_i64_load8_s_local_base as Op,
        decode: decode_i64_load8_s_local_base,
    },
    OpSpec {
        op: op_i64_load8_u_local_base as Op,
        decode: decode_i64_load8_u_local_base,
    },
    OpSpec {
        op: op_i64_load16_s_local_base as Op,
        decode: decode_i64_load16_s_local_base,
    },
    OpSpec {
        op: op_i64_load16_u_local_base as Op,
        decode: decode_i64_load16_u_local_base,
    },
    OpSpec {
        op: op_i64_load32_s_local_base as Op,
        decode: decode_i64_load32_s_local_base,
    },
    OpSpec {
        op: op_i64_load32_u_local_base as Op,
        decode: decode_i64_load32_u_local_base,
    },
    OpSpec {
        op: op_i32_load_const_base as Op,
        decode: decode_i32_load_const_base,
    },
    OpSpec {
        op: op_i32_load_tee4_br_if as Op,
        decode: decode_i32_load_tee4_br_if,
    },
    OpSpec {
        op: op_i32_load_tee4_i32_eqz_br_if as Op,
        decode: decode_i32_load_tee4_i32_eqz_br_if,
    },
    OpSpec {
        op: op_i32_load8_u_tee4_br_if as Op,
        decode: decode_i32_load8_u_tee4_br_if,
    },
    OpSpec {
        op: op_i32_load8_u_tee4_i32_eqz_br_if as Op,
        decode: decode_i32_load8_u_tee4_i32_eqz_br_if,
    },
    OpSpec {
        op: op_i32_load8_s_tee4_br_if as Op,
        decode: decode_i32_load8_s_tee4_br_if,
    },
    OpSpec {
        op: op_i32_load8_s_tee4_i32_eqz_br_if as Op,
        decode: decode_i32_load8_s_tee4_i32_eqz_br_if,
    },
    OpSpec {
        op: op_i32_load16_u_tee4_br_if as Op,
        decode: decode_i32_load16_u_tee4_br_if,
    },
    OpSpec {
        op: op_i32_load16_u_tee4_i32_eqz_br_if as Op,
        decode: decode_i32_load16_u_tee4_i32_eqz_br_if,
    },
    OpSpec {
        op: op_i32_load16_s_tee4_br_if as Op,
        decode: decode_i32_load16_s_tee4_br_if,
    },
    OpSpec {
        op: op_i32_load16_s_tee4_i32_eqz_br_if as Op,
        decode: decode_i32_load16_s_tee4_i32_eqz_br_if,
    },
    OpSpec {
        op: op_i32_load_local_base as Op,
        decode: decode_i32_load_local_base,
    },
    OpSpec {
        op: op_i32_load_local_base_set4 as Op,
        decode: decode_i32_load_local_base_set4,
    },
    OpSpec {
        op: op_i32_load8_u_local_base_set4 as Op,
        decode: decode_i32_load8_u_local_base_set4,
    },
    OpSpec {
        op: op_i32_load8_u_local_base_set4_local_get4 as Op,
        decode: decode_i32_load8_u_local_base_set4_local_get4,
    },
    OpSpec {
        op: op_i32_load8_s_local_base_set4 as Op,
        decode: decode_i32_load8_s_local_base_set4,
    },
    OpSpec {
        op: op_i32_load16_u_local_base_set4 as Op,
        decode: decode_i32_load16_u_local_base_set4,
    },
    OpSpec {
        op: op_i32_load16_s_local_base_set4 as Op,
        decode: decode_i32_load16_s_local_base_set4,
    },
    OpSpec {
        op: op_i32_load_local_base_local_get4 as Op,
        decode: decode_i32_load_local_base_local_get4,
    },
    OpSpec {
        op: op_i32_load8_u_local_base_local_get4 as Op,
        decode: decode_i32_load8_u_local_base_local_get4,
    },
    OpSpec {
        op: op_i32_load8_s_local_base_local_get4 as Op,
        decode: decode_i32_load8_s_local_base_local_get4,
    },
    OpSpec {
        op: op_i32_load16_u_local_base_local_get4 as Op,
        decode: decode_i32_load16_u_local_base_local_get4,
    },
    OpSpec {
        op: op_i32_load16_s_local_base_local_get4 as Op,
        decode: decode_i32_load16_s_local_base_local_get4,
    },
    OpSpec {
        op: op_i32_load8_u_local_base as Op,
        decode: decode_i32_load8_u_local_base,
    },
    OpSpec {
        op: op_i32_load8_s_local_base as Op,
        decode: decode_i32_load8_s_local_base,
    },
    OpSpec {
        op: op_i32_load16_u_local_base as Op,
        decode: decode_i32_load16_u_local_base,
    },
    OpSpec {
        op: op_i32_load16_s_local_base as Op,
        decode: decode_i32_load16_s_local_base,
    },
    OpSpec {
        op: op_i32_load_local_base_tee4 as Op,
        decode: decode_i32_load_local_base_tee4,
    },
    OpSpec {
        op: op_i32_load8_u_local_base_tee4 as Op,
        decode: decode_i32_load8_u_local_base_tee4,
    },
    OpSpec {
        op: op_i32_load8_s_local_base_tee4 as Op,
        decode: decode_i32_load8_s_local_base_tee4,
    },
    OpSpec {
        op: op_i32_load16_u_local_base_tee4 as Op,
        decode: decode_i32_load16_u_local_base_tee4,
    },
    OpSpec {
        op: op_i32_load16_s_local_base_tee4 as Op,
        decode: decode_i32_load16_s_local_base_tee4,
    },
    OpSpec {
        op: op_i32_load_local_base_tee4_local_get4 as Op,
        decode: decode_i32_load_local_base_tee4_local_get4,
    },
    OpSpec {
        op: op_i32_load8_u_local_base_tee4_local_get4 as Op,
        decode: decode_i32_load8_u_local_base_tee4_local_get4,
    },
    OpSpec {
        op: op_i32_load8_s_local_base_tee4_local_get4 as Op,
        decode: decode_i32_load8_s_local_base_tee4_local_get4,
    },
    OpSpec {
        op: op_i32_load16_u_local_base_tee4_local_get4 as Op,
        decode: decode_i32_load16_u_local_base_tee4_local_get4,
    },
    OpSpec {
        op: op_i32_load16_s_local_base_tee4_local_get4 as Op,
        decode: decode_i32_load16_s_local_base_tee4_local_get4,
    },
    OpSpec {
        op: op_i32_load_local_base_set4_i32_load_local_base as Op,
        decode: decode_i32_load_local_base_set4_i32_load_local_base,
    },
    OpSpec {
        op: op_i32_load_local_base_set4_i32_load_local_base_local_eq_br_if as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(
                code,
                cursor,
                RUNTIME_CONT_I32_LOAD_LOCAL_BASE_SET4_I32_LOAD_LOCAL_BASE_LOCAL_EQ_BR_IF,
            )
        },
    },
    OpSpec {
        op: op_i32_load_local_base_set4_i32_load_local_base_local_get4 as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(
                code,
                cursor,
                RUNTIME_CONT_I32_LOAD_LOCAL_BASE_SET4_I32_LOAD_LOCAL_BASE_LOCAL_GET4,
            )
        },
    },
    OpSpec {
        op: op_i32_load_local_base_set4_i32_load8_u_local_base as Op,
        decode: decode_i32_load_local_base_set4_i32_load8_u_local_base,
    },
    OpSpec {
        op: op_i32_load_local_base_set4_i32_load8_s_local_base as Op,
        decode: decode_i32_load_local_base_set4_i32_load8_s_local_base,
    },
    OpSpec {
        op: op_i32_load_local_base_set4_i32_load16_u_local_base as Op,
        decode: decode_i32_load_local_base_set4_i32_load16_u_local_base,
    },
    OpSpec {
        op: op_i32_load_local_base_set4_i32_load16_s_local_base as Op,
        decode: decode_i32_load_local_base_set4_i32_load16_s_local_base,
    },
    OpSpec {
        op: op_i32_load_local_base_set4_i32_load8_u_local_base_local_get4 as Op,
        decode: decode_i32_load_local_base_set4_i32_load8_u_local_base_local_get4,
    },
    OpSpec {
        op: op_i32_load_local_base_set4_i32_load8_u_local_base_local_eq_br_if as Op,
        decode: decode_i32_load_local_base_set4_i32_load8_u_local_base_local_eq_br_if,
    },
    OpSpec {
        op: op_i32_load_local_base_set4_i32_load8_s_local_base_local_eq_br_if as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(
                code,
                cursor,
                RUNTIME_CONT_I32_LOAD_LOCAL_BASE_SET4_I32_LOAD8_S_LOCAL_BASE_LOCAL_EQ_BR_IF,
            )
        },
    },
    OpSpec {
        op: op_i32_load_local_base_set4_i32_load16_u_local_base_local_eq_br_if as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(
                code,
                cursor,
                RUNTIME_CONT_I32_LOAD_LOCAL_BASE_SET4_I32_LOAD16_U_LOCAL_BASE_LOCAL_EQ_BR_IF,
            )
        },
    },
    OpSpec {
        op: op_i32_load_local_base_set4_i32_load16_s_local_base_local_eq_br_if as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(
                code,
                cursor,
                RUNTIME_CONT_I32_LOAD_LOCAL_BASE_SET4_I32_LOAD16_S_LOCAL_BASE_LOCAL_EQ_BR_IF,
            )
        },
    },
    OpSpec {
        op: op_i32_load_local_base_set4_i32_load8_s_local_base_local_get4 as Op,
        decode: decode_i32_load_local_base_set4_i32_load8_s_local_base_local_get4,
    },
    OpSpec {
        op: op_i32_load_local_base_set4_i32_load16_u_local_base_local_get4 as Op,
        decode: decode_i32_load_local_base_set4_i32_load16_u_local_base_local_get4,
    },
    OpSpec {
        op: op_i32_load_local_base_set4_i32_load16_s_local_base_local_get4 as Op,
        decode: decode_i32_load_local_base_set4_i32_load16_s_local_base_local_get4,
    },
    OpSpec {
        op: op_i32_load_local_base_set4_i32_load16_u_local_base_local_eq_search_loop as Op,
        decode: decode_i32_load16_u_search_loop,
    },
    OpSpec {
        op: op_i32_load_local_base_set4_i32_load16_u_local_base_local_eq_search_loop_fallthrough
            as Op,
        decode: decode_i32_load16_u_search_loop_fallthrough,
    },
    OpSpec {
        op: op_i32_load_local_base_set4_i32_load8_u_local_base_local_masked_search_loop as Op,
        decode: decode_i32_load8_u_masked_search_loop,
    },
    OpSpec {
        op: op_i32_load_local_base_set4_i32_load8_u_local_base_local_masked_search_loop_fallthrough
            as Op,
        decode: decode_i32_load8_u_masked_search_loop_fallthrough,
    },
    OpSpec {
        op: op_i32_load_store_local_base_reverse_loop as Op,
        decode: decode_i32_load_store_local_base_reverse_loop,
    },
    OpSpec {
        op: op_i32_load_store_local_base_relink_loop as Op,
        decode: decode_i32_load_store_local_base_relink_loop,
    },
    OpSpec {
        op: op_i32_load16_u_update_store16_local_base_loop as Op,
        decode: decode_i32_load16_u_update_store16_local_base_loop,
    },
    OpSpec {
        op: op_i32_load16_u_local_base_local_get4_i32_load16_u as Op,
        decode: decode_i32_load16_u_local_base_local_get4_i32_load16_u,
    },
    OpSpec {
        op: op_i32_load16_u_local_base_local_get4_i32_load16_u_local_get4 as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(
                code,
                cursor,
                RUNTIME_CONT_I32_LOAD16_U_LOCAL_BASE_LOCAL_GET4_I32_LOAD16_U_LOCAL_GET4,
            )
        },
    },
    OpSpec {
        op: op_i32_load16_s_local_base_local_get4_i32_load16_s as Op,
        decode: decode_i32_load16_s_local_base_local_get4_i32_load16_s,
    },
    OpSpec {
        op: op_i32_load16_s_local_base_local_get4_i32_load16_s_local_get4 as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(
                code,
                cursor,
                RUNTIME_CONT_I32_LOAD16_S_LOCAL_BASE_LOCAL_GET4_I32_LOAD16_S_LOCAL_GET4,
            )
        },
    },
    OpSpec {
        op: op_i32_load_local_base_local_get4_i32_load_tee4_cmp_br_if as Op,
        decode: decode_i32_load_local_base_local_get4_i32_load_tee4_cmp_br_if,
    },
    OpSpec {
        op: op_i32_load16_u_local_scaled_index as Op,
        decode: decode_i32_load16_u_local_scaled_index,
    },
    OpSpec {
        op: op_i32_load16_s_local_scaled_index as Op,
        decode: decode_i32_load16_s_local_scaled_index,
    },
    OpSpec {
        op: op_i32_load8_u_local_scaled_index as Op,
        decode: decode_i32_load8_u_local_scaled_index,
    },
    OpSpec {
        op: op_i32_load8_s_local_scaled_index as Op,
        decode: decode_i32_load8_s_local_scaled_index,
    },
    OpSpec {
        op: op_i32_load_local_scaled_index as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_I32_LOAD_LOCAL_SCALED_INDEX)
        },
    },
    OpSpec {
        op: op_i64_load_local_scaled_index as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_I64_LOAD_LOCAL_SCALED_INDEX)
        },
    },
    OpSpec {
        op: op_i32_load_shared_local_base as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_I32_LOAD_SHARED_LOCAL_BASE)
        },
    },
    OpSpec {
        op: op_i32_load_indexed_local_base as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_I32_LOAD_INDEXED_LOCAL_BASE)
        },
    },
    OpSpec {
        op: op_i32_load_indexed_shared_local_base as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(
                code,
                cursor,
                RUNTIME_CONT_I32_LOAD_INDEXED_SHARED_LOCAL_BASE,
            )
        },
    },
    OpSpec {
        op: op_i32_store as Op,
        decode: decode_i32_store,
    },
    OpSpec {
        op: op_i32_store_indexed_local as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_I32_STORE_INDEXED_LOCAL)
        },
    },
    OpSpec {
        op: op_i32_store_indexed_local_base as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_I32_STORE_INDEXED_LOCAL_BASE)
        },
    },
    OpSpec {
        op: op_i32_store_indexed_local_scaled_index as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(
                code,
                cursor,
                RUNTIME_CONT_I32_STORE_INDEXED_LOCAL_SCALED_INDEX,
            )
        },
    },
    OpSpec {
        op: op_i32_store8 as Op,
        decode: decode_i32_store8,
    },
    OpSpec {
        op: op_i32_store16 as Op,
        decode: decode_i32_store16,
    },
    OpSpec {
        op: op_i32_store_local_base as Op,
        decode: decode_i32_store_local_base,
    },
    OpSpec {
        op: op_i32_store8_local_base as Op,
        decode: decode_i32_store8_local_base,
    },
    OpSpec {
        op: op_i32_store16_local_base as Op,
        decode: decode_i32_store16_local_base,
    },
    OpSpec {
        op: op_i64_store as Op,
        decode: decode_i64_store,
    },
    OpSpec {
        op: op_i64_store8 as Op,
        decode: decode_i64_store8,
    },
    OpSpec {
        op: op_i64_store16 as Op,
        decode: decode_i64_store16,
    },
    OpSpec {
        op: op_i64_store32 as Op,
        decode: decode_i64_store32,
    },
    OpSpec {
        op: op_i64_store_local_base as Op,
        decode: decode_i64_store_local_base,
    },
    OpSpec {
        op: op_i64_store8_local_base as Op,
        decode: decode_i64_store8_local_base,
    },
    OpSpec {
        op: op_i64_store16_local_base as Op,
        decode: decode_i64_store16_local_base,
    },
    OpSpec {
        op: op_i64_store32_local_base as Op,
        decode: decode_i64_store32_local_base,
    },
    OpSpec {
        op: op_f64_store_local_base as Op,
        decode: decode_f64_store_local_base,
    },
    OpSpec {
        op: op_f32_store_const_base_local4 as Op,
        decode: decode_store_const_base_local4,
    },
    OpSpec {
        op: op_i32_store_local_base_local_get4 as Op,
        decode: decode_i32_store_local_base_local_get4,
    },
    OpSpec {
        op: op_i32_store8_local_base_local_get4 as Op,
        decode: decode_i32_store8_local_base_local_get4,
    },
    OpSpec {
        op: op_i32_store16_local_base_local_get4 as Op,
        decode: decode_i32_store16_local_base_local_get4,
    },
    OpSpec {
        op: op_i32_store_local_scaled_index as Op,
        decode: decode_i32_store_local_scaled_index,
    },
    OpSpec {
        op: op_i32_store8_local_scaled_index as Op,
        decode: decode_i32_store8_local_scaled_index,
    },
    OpSpec {
        op: op_i32_store16_local_scaled_index as Op,
        decode: decode_i32_store16_local_scaled_index,
    },
    OpSpec {
        op: op_i32_inc_local_base as Op,
        decode: decode_i32_inc_local_base,
    },
    OpSpec {
        op: op_i32_load_local_base_tee4_i32_load8_u_tee4_br_if as Op,
        decode: decode_i32_load_local_base_tee4_i32_load8_u_tee4_br_if,
    },
    OpSpec {
        op: op_mem_fill_local as Op,
        decode: decode_memory_fill,
    },
    OpSpec {
        op: op_mem_copy_local as Op,
        decode: decode_memory_copy,
    },
    OpSpec {
        op: op_scalar_copy_local_base_run as Op,
        decode: decode_scalar_copy_local_base_run,
    },
    OpSpec {
        op: op_i32_load_local_base_tee4_br_if as Op,
        decode: decode_i32_load_local_base_tee4_br_if,
    },
    OpSpec {
        op: op_i32_load_local_base_tee4_i32_eqz_br_if as Op,
        decode: decode_i32_load_local_base_tee4_i32_eqz_br_if,
    },
    OpSpec {
        op: op_i32_load8_u_local_base_tee4_br_if as Op,
        decode: decode_i32_load8_u_local_base_tee4_br_if,
    },
    OpSpec {
        op: op_i32_load8_u_local_base_tee4_i32_eqz_br_if as Op,
        decode: decode_i32_load8_u_local_base_tee4_i32_eqz_br_if,
    },
    OpSpec {
        op: op_i32_load8_s_local_base_tee4_br_if as Op,
        decode: decode_i32_load8_s_local_base_tee4_br_if,
    },
    OpSpec {
        op: op_i32_load8_s_local_base_tee4_i32_eqz_br_if as Op,
        decode: decode_i32_load8_s_local_base_tee4_i32_eqz_br_if,
    },
    OpSpec {
        op: op_i32_load16_u_local_base_tee4_br_if as Op,
        decode: decode_i32_load16_u_local_base_tee4_br_if,
    },
    OpSpec {
        op: op_i32_load16_u_local_base_tee4_i32_eqz_br_if as Op,
        decode: decode_i32_load16_u_local_base_tee4_i32_eqz_br_if,
    },
    OpSpec {
        op: op_i32_load16_s_local_base_tee4_br_if as Op,
        decode: decode_i32_load16_s_local_base_tee4_br_if,
    },
    OpSpec {
        op: op_i32_load16_s_local_base_tee4_i32_eqz_br_if as Op,
        decode: decode_i32_load16_s_local_base_tee4_i32_eqz_br_if,
    },
    OpSpec {
        op: op_local_get4_i32_const_add_set4_i32_load8_u_local_base_tee4_i32_eqz_br_if as Op,
        decode: decode_local_add_set_load8_eqz_br_if,
    },
    OpSpec {
        op: op_br as Op,
        decode: decode_branch,
    },
    OpSpec {
        op: op_return as Op,
        decode: decode_branch,
    },
    OpSpec {
        op: op_br_if as Op,
        decode: decode_br_if,
    },
    OpSpec {
        op: op_local_get4_br_if as Op,
        decode: decode_local_get4_br_if,
    },
    OpSpec {
        op: op_local_get4_i32_const_add_br_if as Op,
        decode: decode_local_get4_i32_const_add_br_if,
    },
    OpSpec {
        op: op_local_get4_local_get4_i32_add_br_if as Op,
        decode: decode_local_get4_local_get4_i32_add_br_if,
    },
    OpSpec {
        op: op_local_get4_i32_eqz_br_if as Op,
        decode: decode_local_get4_i32_eqz_br_if,
    },
    OpSpec {
        op: op_local_get4_i32_const_compare_br_if as Op,
        decode: decode_local_get4_i32_const_compare_br_if,
    },
    OpSpec {
        op: op_local_get4_local_get4_compare_br_if as Op,
        decode: decode_local_get4_local_get4_compare_br_if,
    },
    OpSpec {
        op: op_local_get4_i32_const_and_br_if as Op,
        decode: decode_local_get4_i32_const_and_br_if,
    },
    OpSpec {
        op: op_local_get4_i32_const_and_eqz_br_if as Op,
        decode: decode_local_get4_i32_const_and_eqz_br_if,
    },
    OpSpec {
        op: op_local_get4_i32_const_and_i32_const_compare_br_if as Op,
        decode: decode_local_get4_i32_const_and_i32_const_compare_br_if,
    },
    OpSpec {
        op: op_local_get4_i32_const_and_tee4_i32_const_eq_br_if as Op,
        decode: decode_local_get4_i32_const_and_tee4_i32_const_eq_br_if,
    },
    OpSpec {
        op: op_local_get4_set4_local_get4_i32_const_compare_br_if as Op,
        decode: decode_local_get4_set4_local_get4_i32_const_compare_br_if,
    },
    OpSpec {
        op: op_local_get4_i32_const_add_i32_const_and_i32_const_compare_br_if as Op,
        decode: decode_local_get4_i32_const_add_i32_const_and_i32_const_compare_br_if,
    },
    OpSpec {
        op: op_local_get4_i32_const_add_tee4_br_if as Op,
        decode: decode_local_get4_i32_const_add_tee4_br_if,
    },
    OpSpec {
        op: op_local_get4_br_table as Op,
        decode: decode_local_get4_br_table,
    },
    OpSpec {
        op: op_local_get4_i32_const_add_br_table as Op,
        decode: decode_local_get4_i32_const_add_br_table,
    },
    OpSpec {
        op: op_br_table as Op,
        decode: decode_br_table,
    },
    OpSpec {
        op: op_if as Op,
        decode: decode_if,
    },
    OpSpec {
        op: op_else as Op,
        decode: decode_else,
    },
    OpSpec {
        op: op_end as Op,
        decode: decode_end,
    },
    OpSpec {
        op: special_block_return as Op,
        decode: decode_block_return,
    },
    OpSpec {
        op: op_loop as Op,
        decode: decode_loop,
    },
    OpSpec {
        op: special_function_return as Op,
        decode: decode_function_return,
    },
    OpSpec {
        op: special_function_vm_end as Op,
        decode: decode_function_vm_end,
    },
    OpSpec {
        op: op_i32_crc16_update16 as Op,
        decode: decode_i32_crc16_update16,
    },
    OpSpec {
        op: op_i32_crc16_update16_masked as Op,
        decode: decode_i32_crc16_update16_masked,
    },
    OpSpec {
        op: op_i32_core_state_benchmark as Op,
        decode: decode_i32_core_state_benchmark,
    },
    OpSpec {
        op: op_i32_list_crc_pair_loop as Op,
        decode: decode_i32_list_crc_pair_loop,
    },
    OpSpec {
        op: op_i32_list_crc_summary as Op,
        decode: decode_i32_list_crc_summary,
    },
    OpSpec {
        op: op_i32_select_bit_step4 as Op,
        decode: decode_i32_select_bit_step4,
    },
    OpSpec {
        op: op_i32_select_bit_step4_run as Op,
        decode: decode_i32_select_bit_step4_run,
    },
    OpSpec {
        op: op_call_i32_crc16_update16 as Op,
        decode: decode_call_i32_crc16_update16,
    },
    OpSpec {
        op: op_call_i32_list_crc_summary as Op,
        decode: decode_call_i32_list_crc_summary,
    },
    OpSpec {
        op: op_call as Op,
        decode: decode_call,
    },
    OpSpec {
        op: op_call_jit_lazy as Op,
        decode: decode_call,
    },
    OpSpec {
        op: op_call_import as Op,
        decode: decode_call,
    },
    OpSpec {
        op: op_call_indirect as Op,
        decode: decode_call_indirect,
    },
    OpSpec {
        op: op_return_call as Op,
        decode: decode_return_call,
    },
    OpSpec {
        op: op_return_call_jit_lazy as Op,
        decode: decode_return_call,
    },
    OpSpec {
        op: op_return_call_import as Op,
        decode: decode_return_call,
    },
    OpSpec {
        op: op_return_call_indirect as Op,
        decode: decode_return_call_indirect,
    },
    #[cfg(feature = "threads")]
    OpSpec {
        op: op_atomic_fence as Op,
        decode: decode_atomic_fence,
    },
    #[cfg(feature = "threads")]
    OpSpec {
        op: op_atomic_fence_shared as Op,
        decode: decode_atomic_fence_shared,
    },
    OpSpec {
        op: op_call_cached_u16_low7_guard as Op,
        decode: decode_call_cached_u16_low7_guard,
    },
    OpSpec {
        op: op_call_i32_crc16_update16_masked as Op,
        decode: decode_call_i32_crc16_update16_masked,
    },
    OpSpec {
        op: op_call_i32_numeric_token_state_transition as Op,
        decode: decode_call_i32_numeric_token_state_transition,
    },
    OpSpec {
        op: op_data_drop as Op,
        decode: decode_data_drop,
    },
    OpSpec {
        op: op_elem_drop as Op,
        decode: decode_elem_drop,
    },
    OpSpec {
        op: op_f32_ceil as Op,
        decode: decode_f32_ceil,
    },
    OpSpec {
        op: op_f32_convert_i32_s as Op,
        decode: decode_f32_convert_i32_s,
    },
    OpSpec {
        op: op_f32_convert_i32_u as Op,
        decode: decode_f32_convert_i32_u,
    },
    OpSpec {
        op: op_f32_convert_i64_s as Op,
        decode: decode_f32_convert_i64_s,
    },
    OpSpec {
        op: op_f32_convert_i64_u as Op,
        decode: decode_f32_convert_i64_u,
    },
    OpSpec {
        op: op_f32_copysign as Op,
        decode: decode_f32_copysign,
    },
    OpSpec {
        op: op_f32_demote_f64 as Op,
        decode: decode_f32_demote_f64,
    },
    OpSpec {
        op: op_f32_floor as Op,
        decode: decode_f32_floor,
    },
    OpSpec {
        op: op_f32_load as Op,
        decode: decode_f32_load,
    },
    OpSpec {
        op: op_f32_load_const_base as Op,
        decode: decode_f32_load_const_base,
    },
    OpSpec {
        op: op_f32_max as Op,
        decode: decode_f32_max,
    },
    OpSpec {
        op: op_f32_min as Op,
        decode: decode_f32_min,
    },
    OpSpec {
        op: op_f32_nearest as Op,
        decode: decode_f32_nearest,
    },
    OpSpec {
        op: op_f32_store as Op,
        decode: decode_f32_store,
    },
    OpSpec {
        op: op_f32_store_local_base as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_F32_STORE_LOCAL_BASE)
        },
    },
    OpSpec {
        op: op_f64_copysign as Op,
        decode: decode_f64_copysign,
    },
    OpSpec {
        op: op_f64_load as Op,
        decode: decode_f64_load,
    },
    OpSpec {
        op: op_f64_max as Op,
        decode: decode_f64_max,
    },
    OpSpec {
        op: op_f64_min as Op,
        decode: decode_f64_min,
    },
    OpSpec {
        op: op_f64_promote_f32 as Op,
        decode: decode_f64_promote_f32,
    },
    OpSpec {
        op: op_f64_store as Op,
        decode: decode_f64_store,
    },
    OpSpec {
        op: op_f64_store_const_base_local8 as Op,
        decode: decode_store_const_base_local8,
    },
    OpSpec {
        op: op_f64_store_local_scaled_index as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(
                code,
                cursor,
                RUNTIME_CONT_F64_STORE_LOCAL_SCALED_INDEX,
            )
        },
    },
    OpSpec {
        op: op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if as Op,
        decode: decode_i32_guarded_load8_update_br_if,
    },
    OpSpec {
        op: op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_false_local_get4_br_table as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(
                code,
                cursor,
                RUNTIME_CONT_I32_GUARDED_LOAD8_UPDATE_BR_IF_FALSE_BR_TABLE,
            )
        },
    },
    OpSpec {
        op: op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_taken_const_compare_br_table as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(
                code,
                cursor,
                RUNTIME_CONT_I32_GUARDED_LOAD8_UPDATE_BR_IF_TAKEN_CONST_CMP_BR_TABLE,
            )
        },
    },
    OpSpec {
        op: op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_taken_local_get4_br_table as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(
                code,
                cursor,
                RUNTIME_CONT_I32_GUARDED_LOAD8_UPDATE_BR_IF_TAKEN_BR_TABLE,
            )
        },
    },
    OpSpec {
        op: op_i32_inc_local_base_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_I32_INC_LOAD8_UPDATE_BR_IF)
        },
    },
    OpSpec {
        op: op_i32_load16_s_dot4_local_base_loop as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_I32_LOAD16_S_DOT4_LOOP)
        },
    },
    OpSpec {
        op: op_i32_load16_s_mul_add_local_base_delta_loop as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(
                code,
                cursor,
                RUNTIME_CONT_I32_LOAD16_S_MUL_ADD_DELTA_LOOP,
            )
        },
    },
    OpSpec {
        op: op_i32_load16_s_mul_add_local_base_loop as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_I32_LOAD16_S_MUL_ADD_LOOP)
        },
    },
    OpSpec {
        op: op_i32_load16_u_bitmix_acc_local_base_delta_loop as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(
                code,
                cursor,
                RUNTIME_CONT_I32_LOAD16_U_BITMIX_DELTA_LOOP,
            )
        },
    },
    OpSpec {
        op: op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if as Op,
        decode: decode_i32_load8_update_br_if,
    },
    OpSpec {
        op: op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_fallthrough_local_get4 as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(
                code,
                cursor,
                RUNTIME_CONT_I32_LOAD8_UPDATE_BR_IF_FALLTHROUGH_LOCAL_GET4,
            )
        },
    },
    OpSpec {
        op: op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_taken_local_get4 as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(
                code,
                cursor,
                RUNTIME_CONT_I32_LOAD8_UPDATE_BR_IF_TAKEN_LOCAL_GET4,
            )
        },
    },
    OpSpec {
        op: op_i32_load_const_base_local_get4_i32_add_set4 as Op,
        decode: decode_i32_load_const_base_local_get4_i32_add_set4,
    },
    OpSpec {
        op: op_i32_load_local_base_set4_i32_load8_u_local_base_local_masked_compare_br_if as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(
                code,
                cursor,
                RUNTIME_CONT_I32_LOAD_MASKED_COMPARE_BR_IF,
            )
        },
    },
    OpSpec {
        op: op_i32_load_store_local_base_local_get4 as Op,
        decode: decode_i32_load_store_local_base_local_get4,
    },
    OpSpec {
        op: op_i32_matrix_i16_crc_summary as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_I32_MATRIX_I16_CRC_SUMMARY)
        },
    },
    OpSpec {
        op: op_i32_numeric_token_state_transition as Op,
        decode: decode_i32_numeric_token_state_transition,
    },
    OpSpec {
        op: op_i32_popcnt as Op,
        decode: decode_i32_popcnt,
    },
    OpSpec {
        op: op_i32_store_const_base_local4 as Op,
        decode: decode_store_const_base_local4,
    },
    OpSpec {
        op: op_i32_sum_clip_local_base_loop as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_I32_SUM_CLIP_LOOP)
        },
    },
    OpSpec {
        op: op_i32_trunc_f32_s as Op,
        decode: decode_i32_trunc_f32_s,
    },
    OpSpec {
        op: op_i32_trunc_f32_u as Op,
        decode: decode_i32_trunc_f32_u,
    },
    OpSpec {
        op: op_i32_trunc_f64_s as Op,
        decode: decode_i32_trunc_f64_s,
    },
    OpSpec {
        op: op_i32_trunc_f64_u as Op,
        decode: decode_i32_trunc_f64_u,
    },
    OpSpec {
        op: op_i64_load_const_base as Op,
        decode: decode_i64_load_const_base,
    },
    OpSpec {
        op: op_i64_store_const_base_local8 as Op,
        decode: decode_store_const_base_local8,
    },
    OpSpec {
        op: op_i64_trunc_f32_s as Op,
        decode: decode_i64_trunc_f32_s,
    },
    OpSpec {
        op: op_i64_trunc_f32_u as Op,
        decode: decode_i64_trunc_f32_u,
    },
    OpSpec {
        op: op_i64_trunc_f64_s as Op,
        decode: decode_i64_trunc_f64_s,
    },
    OpSpec {
        op: op_i64_trunc_f64_u as Op,
        decode: decode_i64_trunc_f64_u,
    },
    OpSpec {
        op: op_i64_trunc_sat_f32_s as Op,
        decode: decode_i64_trunc_sat_f32_s,
    },
    OpSpec {
        op: op_i64_trunc_sat_f32_u as Op,
        decode: decode_i64_trunc_sat_f32_u,
    },
    OpSpec {
        op: op_i64_trunc_sat_f64_s as Op,
        decode: decode_i64_trunc_sat_f64_s,
    },
    OpSpec {
        op: op_i64_trunc_sat_f64_u as Op,
        decode: decode_i64_trunc_sat_f64_u,
    },
    #[cfg(feature = "simd")]
    OpSpec {
        op: op_i8x16_extract_lane_s as Op,
        decode: decode_i8x16_extract_lane_s,
    },
    #[cfg(feature = "simd")]
    OpSpec {
        op: f32x4_replace_lane as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_SIMD_F32X4_REPLACE_LANE)
        },
    },
    #[cfg(feature = "simd")]
    OpSpec {
        op: f64x2_replace_lane as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_SIMD_F64X2_REPLACE_LANE)
        },
    },
    #[cfg(feature = "simd")]
    OpSpec {
        op: i16x8_replace_lane as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_SIMD_I16X8_REPLACE_LANE)
        },
    },
    #[cfg(feature = "simd")]
    OpSpec {
        op: i16x8_shl as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_SIMD_I16X8_SHL)
        },
    },
    #[cfg(feature = "simd")]
    OpSpec {
        op: i16x8_shr as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_SIMD_I16X8_SHR)
        },
    },
    #[cfg(feature = "simd")]
    OpSpec {
        op: i32x4_replace_lane as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_SIMD_I32X4_REPLACE_LANE)
        },
    },
    #[cfg(feature = "simd")]
    OpSpec {
        op: i32x4_shl as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_SIMD_I32X4_SHL)
        },
    },
    #[cfg(feature = "simd")]
    OpSpec {
        op: i32x4_shr as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_SIMD_I32X4_SHR)
        },
    },
    #[cfg(feature = "simd")]
    OpSpec {
        op: i64x2_replace_lane as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_SIMD_I64X2_REPLACE_LANE)
        },
    },
    #[cfg(feature = "simd")]
    OpSpec {
        op: i64x2_shl as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_SIMD_I64X2_SHL)
        },
    },
    #[cfg(feature = "simd")]
    OpSpec {
        op: i64x2_shr as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_SIMD_I64X2_SHR)
        },
    },
    #[cfg(feature = "simd")]
    OpSpec {
        op: i8x16_replace_lane as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_SIMD_I8X16_REPLACE_LANE)
        },
    },
    #[cfg(feature = "simd")]
    OpSpec {
        op: i8x16_shl as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_SIMD_I8X16_SHL)
        },
    },
    #[cfg(feature = "simd")]
    OpSpec {
        op: i8x16_shr as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_SIMD_I8X16_SHR)
        },
    },
    #[cfg(feature = "simd")]
    OpSpec {
        op: i8x16_shuffle as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_SIMD_I8X16_SHUFFLE)
        },
    },
    #[cfg(feature = "simd")]
    OpSpec {
        op: i8x16_swizzle as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_SIMD_I8X16_SWIZZLE)
        },
    },
    #[cfg(feature = "simd")]
    OpSpec {
        op: op_v128_load as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_SIMD_V128_LOAD)
        },
    },
    #[cfg(feature = "simd")]
    OpSpec {
        op: op_v128_load_indexed_local as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(
                code,
                cursor,
                RUNTIME_CONT_SIMD_V128_LOAD_INDEXED_LOCAL,
            )
        },
    },
    #[cfg(feature = "simd")]
    OpSpec {
        op: op_v128_load_indexed_shared as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(
                code,
                cursor,
                RUNTIME_CONT_SIMD_V128_LOAD_INDEXED_SHARED,
            )
        },
    },
    #[cfg(feature = "simd")]
    OpSpec {
        op: op_v128_load_shared as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_SIMD_V128_LOAD_SHARED)
        },
    },
    OpSpec {
        op: op_local_get4_i32_inc_local_base as Op,
        decode: decode_local_get4_i32_inc_local_base,
    },
    OpSpec {
        op: op_local_get4_i32_inc_local_base_i32_load8_u_local_base_set4 as Op,
        decode: decode_local_get4_i32_inc_local_base_i32_load8_u_local_base_set4,
    },
    OpSpec {
        op: op_local_get4_i32_load8_u_local_base_set4 as Op,
        decode: decode_local_get4_i32_load8_u_local_base_set4,
    },
    OpSpec {
        op: op_local_get4_i32_load_local_base_i32_add_set4 as Op,
        decode: decode_local_get4_i32_load_local_base_i32_add_set4,
    },
    OpSpec {
        op: op_local_get4_i32_load_local_base_i32_add_tee4 as Op,
        decode: decode_local_get4_i32_load_local_base_i32_add_tee4,
    },
    OpSpec {
        op: op_local_get4x3_i32_add_const_binop_i32_add_set4 as Op,
        decode: decode_local_get4x3_i32_add_const_binop_i32_add_set4,
    },
    OpSpec {
        op: op_local_get4x3_i32_add_const_binop_i32_add_tee4 as Op,
        decode: decode_local_get4x3_i32_add_const_binop_i32_add_tee4,
    },
    OpSpec {
        op: op_local_get4x3_i32_add_const_binop_i32_add_tee4_i32_const_store as Op,
        decode: decode_local_get4x3_i32_add_const_binop_i32_add_tee4_i32_const_store,
    },
    OpSpec {
        op: op_mem_copy as Op,
        decode: decode_memory_copy,
    },
    OpSpec {
        op: op_mem_copy_indexed_local_local as Op,
        decode: decode_mem_copy_indexed_local_local,
    },
    OpSpec {
        op: op_mem_copy_indexed_local_shared as Op,
        decode: decode_mem_copy_indexed_local_shared,
    },
    OpSpec {
        op: op_mem_copy_indexed_shared_local as Op,
        decode: decode_mem_copy_indexed_shared_local,
    },
    OpSpec {
        op: op_mem_copy_indexed_shared_shared as Op,
        decode: decode_mem_copy_indexed_shared_shared,
    },
    OpSpec {
        op: op_mem_copy_shared as Op,
        decode: decode_mem_copy_shared,
    },
    OpSpec {
        op: op_mem_fill as Op,
        decode: decode_memory_fill,
    },
    OpSpec {
        op: op_mem_fill_indexed_local as Op,
        decode: decode_mem_fill_indexed_local,
    },
    OpSpec {
        op: op_mem_fill_indexed_shared as Op,
        decode: decode_mem_fill_indexed_shared,
    },
    OpSpec {
        op: op_mem_fill_shared as Op,
        decode: decode_mem_fill_shared,
    },
    OpSpec {
        op: op_mem_grow as Op,
        decode: decode_mem_grow,
    },
    OpSpec {
        op: op_mem_grow_indexed_local as Op,
        decode: decode_mem_grow_indexed_local,
    },
    OpSpec {
        op: op_mem_grow_indexed_shared as Op,
        decode: decode_mem_grow_indexed_shared,
    },
    OpSpec {
        op: op_mem_grow_shared as Op,
        decode: decode_mem_grow_shared,
    },
    OpSpec {
        op: op_mem_init as Op,
        decode: decode_mem_init,
    },
    OpSpec {
        op: op_mem_init_indexed_local as Op,
        decode: decode_mem_init_indexed_local,
    },
    OpSpec {
        op: op_mem_init_indexed_shared as Op,
        decode: decode_mem_init_indexed_shared,
    },
    OpSpec {
        op: op_mem_init_shared as Op,
        decode: decode_mem_init_shared,
    },
    OpSpec {
        op: op_mem_size as Op,
        decode: decode_mem_size,
    },
    OpSpec {
        op: op_mem_size_indexed_local as Op,
        decode: decode_mem_size_indexed_local,
    },
    OpSpec {
        op: op_mem_size_indexed_shared as Op,
        decode: decode_mem_size_indexed_shared,
    },
    OpSpec {
        op: op_mem_size_shared as Op,
        decode: decode_mem_size_shared,
    },
    #[cfg(feature = "threads")]
    OpSpec {
        op: op_memory_atomic_notify as Op,
        decode: decode_atomic_notify_local,
    },
    #[cfg(feature = "threads")]
    OpSpec {
        op: op_memory_atomic_notify_unshared as Op,
        decode: decode_atomic_notify_local,
    },
    #[cfg(feature = "threads")]
    OpSpec {
        op: op_memory_atomic_notify_indexed_shared as Op,
        decode: decode_atomic_notify_indexed_shared,
    },
    #[cfg(feature = "threads")]
    OpSpec {
        op: op_memory_atomic_notify_indexed_unshared as Op,
        decode: decode_atomic_notify_indexed_local,
    },
    #[cfg(feature = "threads")]
    OpSpec {
        op: op_memory_atomic_notify_shared as Op,
        decode: decode_atomic_notify_shared,
    },
    #[cfg(feature = "threads")]
    OpSpec {
        op: op_memory_atomic_wait32 as Op,
        decode: decode_atomic_wait32_local,
    },
    #[cfg(feature = "threads")]
    OpSpec {
        op: op_memory_atomic_wait32_unshared as Op,
        decode: decode_atomic_wait32_local,
    },
    #[cfg(feature = "threads")]
    OpSpec {
        op: op_memory_atomic_wait32_indexed_shared as Op,
        decode: decode_atomic_wait32_indexed_shared,
    },
    #[cfg(feature = "threads")]
    OpSpec {
        op: op_memory_atomic_wait32_indexed_unshared as Op,
        decode: decode_atomic_wait32_indexed_local,
    },
    #[cfg(feature = "threads")]
    OpSpec {
        op: op_memory_atomic_wait32_shared as Op,
        decode: decode_atomic_wait32_shared,
    },
    #[cfg(feature = "threads")]
    OpSpec {
        op: op_memory_atomic_wait64 as Op,
        decode: decode_atomic_wait64_local,
    },
    #[cfg(feature = "threads")]
    OpSpec {
        op: op_memory_atomic_wait64_indexed_shared as Op,
        decode: decode_atomic_wait64_indexed_shared,
    },
    #[cfg(feature = "threads")]
    OpSpec {
        op: op_memory_atomic_wait64_indexed_unshared as Op,
        decode: decode_atomic_wait64_indexed_local,
    },
    #[cfg(feature = "threads")]
    OpSpec {
        op: op_memory_atomic_wait64_shared as Op,
        decode: decode_atomic_wait64_shared,
    },
    OpSpec {
        op: op_ref_func as Op,
        decode: decode_ref_func,
    },
    OpSpec {
        op: op_ref_is_null as Op,
        decode: decode_ref_is_null,
    },
    OpSpec {
        op: op_ref_null as Op,
        decode: decode_ref_null,
    },
    OpSpec {
        op: op_table_copy as Op,
        decode: decode_table_copy,
    },
    OpSpec {
        op: op_table_fill as Op,
        decode: decode_table_fill,
    },
    OpSpec {
        op: op_table_get as Op,
        decode: decode_table_get,
    },
    OpSpec {
        op: op_table_grow as Op,
        decode: decode_table_grow,
    },
    OpSpec {
        op: op_table_init as Op,
        decode: decode_table_init,
    },
    OpSpec {
        op: op_table_set as Op,
        decode: decode_table_set,
    },
    OpSpec {
        op: op_table_size as Op,
        decode: decode_table_size,
    },
    OpSpec {
        op: op_unreachable as Op,
        decode: decode_unreachable,
    },
    #[cfg(feature = "simd")]
    OpSpec {
        op: op_v128_bitselect as Op,
        decode: decode_v128_bitselect,
    },
    OpSpec {
        op: special_start_function_call as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_START_FUNCTION_CALL)
        },
    },
    OpSpec {
        op: special_start_jit_function_call as Op,
        decode: |code, cursor| {
            decode_runtime_continuation_stub(code, cursor, RUNTIME_CONT_START_JIT_FUNCTION_CALL)
        },
    },
];

pub(super) fn decode_baseline_op(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    let op = unsafe { instr(code, cursor)?.op };
    if let Some(spec) = OP_SPECS
        .iter()
        .find(|spec| std::ptr::fn_addr_eq(op, spec.op))
    {
        return (spec.decode)(code, cursor);
    }
    Err(())
}

const RUNTIME_STUB_DATA_DROP: u32 = 0;
const RUNTIME_STUB_ELEM_DROP: u32 = 1;
const RUNTIME_STUB_MEM_INIT_LOCAL: u32 = 2;
const RUNTIME_STUB_MEM_INIT_SHARED: u32 = 3;
const RUNTIME_STUB_MEM_INIT_INDEXED_LOCAL: u32 = 4;
const RUNTIME_STUB_MEM_INIT_INDEXED_SHARED: u32 = 5;
const RUNTIME_STUB_MEM_COPY_SHARED: u32 = 6;
const RUNTIME_STUB_MEM_COPY_INDEXED_LOCAL_LOCAL: u32 = 7;
const RUNTIME_STUB_MEM_COPY_INDEXED_LOCAL_SHARED: u32 = 8;
const RUNTIME_STUB_MEM_COPY_INDEXED_SHARED_LOCAL: u32 = 9;
const RUNTIME_STUB_MEM_COPY_INDEXED_SHARED_SHARED: u32 = 10;
const RUNTIME_STUB_MEM_FILL_SHARED: u32 = 11;
const RUNTIME_STUB_MEM_FILL_INDEXED_LOCAL: u32 = 12;
const RUNTIME_STUB_MEM_FILL_INDEXED_SHARED: u32 = 13;
const RUNTIME_STUB_MEM_SIZE_INDEXED_LOCAL: u32 = 14;
const RUNTIME_STUB_MEM_SIZE_INDEXED_SHARED: u32 = 15;
const RUNTIME_STUB_MEM_GROW_INDEXED_LOCAL: u32 = 16;
const RUNTIME_STUB_MEM_GROW_INDEXED_SHARED: u32 = 17;
const RUNTIME_STUB_TABLE_GET: u32 = 18;
const RUNTIME_STUB_TABLE_SET: u32 = 19;
const RUNTIME_STUB_TABLE_INIT: u32 = 20;
const RUNTIME_STUB_TABLE_COPY: u32 = 21;
const RUNTIME_STUB_TABLE_GROW: u32 = 22;
const RUNTIME_STUB_TABLE_SIZE: u32 = 23;
const RUNTIME_STUB_TABLE_FILL: u32 = 24;
const RUNTIME_STUB_CALL_NUMERIC_TOKEN_STATE_TRANSITION: u32 = 25;
const RUNTIME_STUB_CALL_CACHED_U16_LOW7_GUARD: u32 = 26;
const RUNTIME_STUB_I8X16_EXTRACT_LANE_S: u32 = 27;
const RUNTIME_STUB_V128_BITSELECT: u32 = 28;
const RUNTIME_STUB_ATOMIC_NOTIFY_LOCAL: u32 = 29;
const RUNTIME_STUB_ATOMIC_NOTIFY_SHARED: u32 = 30;
const RUNTIME_STUB_ATOMIC_NOTIFY_INDEXED_LOCAL: u32 = 31;
const RUNTIME_STUB_ATOMIC_NOTIFY_INDEXED_SHARED: u32 = 32;
const RUNTIME_STUB_ATOMIC_WAIT32_LOCAL: u32 = 33;
const RUNTIME_STUB_ATOMIC_WAIT32_SHARED: u32 = 34;
const RUNTIME_STUB_ATOMIC_WAIT32_INDEXED_LOCAL: u32 = 35;
const RUNTIME_STUB_ATOMIC_WAIT32_INDEXED_SHARED: u32 = 36;
const RUNTIME_STUB_ATOMIC_WAIT64_LOCAL: u32 = 37;
const RUNTIME_STUB_ATOMIC_WAIT64_SHARED: u32 = 38;
const RUNTIME_STUB_ATOMIC_WAIT64_INDEXED_LOCAL: u32 = 39;
const RUNTIME_STUB_ATOMIC_WAIT64_INDEXED_SHARED: u32 = 40;

const RUNTIME_CONT_I32_GUARDED_LOAD8_UPDATE_BR_IF_FALSE_BR_TABLE: u32 = 1;
const RUNTIME_CONT_I32_GUARDED_LOAD8_UPDATE_BR_IF_TAKEN_CONST_CMP_BR_TABLE: u32 = 2;
const RUNTIME_CONT_I32_GUARDED_LOAD8_UPDATE_BR_IF_TAKEN_BR_TABLE: u32 = 3;
const RUNTIME_CONT_I32_INC_LOAD8_UPDATE_BR_IF: u32 = 4;
const RUNTIME_CONT_I32_LOAD16_S_DOT4_LOOP: u32 = 5;
const RUNTIME_CONT_I32_LOAD16_S_MUL_ADD_DELTA_LOOP: u32 = 6;
const RUNTIME_CONT_I32_LOAD16_S_MUL_ADD_LOOP: u32 = 7;
const RUNTIME_CONT_I32_LOAD16_U_BITMIX_DELTA_LOOP: u32 = 8;
const RUNTIME_CONT_I32_LOAD8_UPDATE_BR_IF_FALLTHROUGH_LOCAL_GET4: u32 = 10;
const RUNTIME_CONT_I32_LOAD8_UPDATE_BR_IF_TAKEN_LOCAL_GET4: u32 = 11;
const RUNTIME_CONT_I32_LOAD_MASKED_COMPARE_BR_IF: u32 = 12;
const RUNTIME_CONT_I32_MATRIX_I16_CRC_SUMMARY: u32 = 13;
const RUNTIME_CONT_I32_SUM_CLIP_LOOP: u32 = 14;
const RUNTIME_CONT_START_FUNCTION_CALL: u32 = 15;
const RUNTIME_CONT_START_JIT_FUNCTION_CALL: u32 = 16;
const RUNTIME_CONT_I32_LOAD_LOCAL_BASE_SET4_I32_LOAD_LOCAL_BASE_LOCAL_EQ_BR_IF: u32 = 21;
const RUNTIME_CONT_I32_LOAD_LOCAL_BASE_SET4_I32_LOAD8_S_LOCAL_BASE_LOCAL_EQ_BR_IF: u32 = 22;
const RUNTIME_CONT_I32_LOAD_LOCAL_BASE_SET4_I32_LOAD16_U_LOCAL_BASE_LOCAL_EQ_BR_IF: u32 = 23;
const RUNTIME_CONT_I32_LOAD_LOCAL_BASE_SET4_I32_LOAD16_S_LOCAL_BASE_LOCAL_EQ_BR_IF: u32 = 24;
const RUNTIME_CONT_I32_LOAD16_U_LOCAL_BASE_LOCAL_GET4_I32_LOAD16_U_LOCAL_GET4: u32 = 25;
const RUNTIME_CONT_I32_LOAD16_S_LOCAL_BASE_LOCAL_GET4_I32_LOAD16_S_LOCAL_GET4: u32 = 26;
const RUNTIME_CONT_LOCAL_GET4_I32_LOAD16_U_LOCAL_BASE_LOCAL_GET4_I32_LOAD16_U: u32 = 27;
const RUNTIME_CONT_LOCAL_GET4_I32_LOAD16_S_LOCAL_BASE_LOCAL_GET4_I32_LOAD16_S: u32 = 28;
const RUNTIME_CONT_F32_STORE_LOCAL_BASE: u32 = 29;
const RUNTIME_CONT_F64_STORE_LOCAL_SCALED_INDEX: u32 = 30;
const RUNTIME_CONT_I32_LOAD_INDEXED_LOCAL_BASE: u32 = 31;
const RUNTIME_CONT_I32_LOAD_INDEXED_SHARED_LOCAL_BASE: u32 = 32;
const RUNTIME_CONT_I32_LOAD_LOCAL_BASE_SET4_I32_LOAD_LOCAL_BASE_LOCAL_GET4: u32 = 33;
const RUNTIME_CONT_I32_LOAD_LOCAL_SCALED_INDEX: u32 = 34;
const RUNTIME_CONT_I32_LOAD_SHARED_LOCAL_BASE: u32 = 35;
const RUNTIME_CONT_I32_STORE_INDEXED_LOCAL: u32 = 36;
const RUNTIME_CONT_I32_STORE_INDEXED_LOCAL_BASE: u32 = 37;
const RUNTIME_CONT_I32_STORE_INDEXED_LOCAL_SCALED_INDEX: u32 = 38;
const RUNTIME_CONT_I64_LOAD_LOCAL_SCALED_INDEX: u32 = 39;
const RUNTIME_CONT_SIMD_F32X4_REPLACE_LANE: u32 = 40;
const RUNTIME_CONT_SIMD_F64X2_REPLACE_LANE: u32 = 41;
const RUNTIME_CONT_SIMD_I16X8_REPLACE_LANE: u32 = 42;
const RUNTIME_CONT_SIMD_I16X8_SHL: u32 = 43;
const RUNTIME_CONT_SIMD_I16X8_SHR: u32 = 44;
const RUNTIME_CONT_SIMD_I32X4_REPLACE_LANE: u32 = 45;
const RUNTIME_CONT_SIMD_I32X4_SHL: u32 = 46;
const RUNTIME_CONT_SIMD_I32X4_SHR: u32 = 47;
const RUNTIME_CONT_SIMD_I64X2_REPLACE_LANE: u32 = 48;
const RUNTIME_CONT_SIMD_I64X2_SHL: u32 = 49;
const RUNTIME_CONT_SIMD_I64X2_SHR: u32 = 50;
const RUNTIME_CONT_SIMD_I8X16_REPLACE_LANE: u32 = 51;
const RUNTIME_CONT_SIMD_I8X16_SHL: u32 = 52;
const RUNTIME_CONT_SIMD_I8X16_SHR: u32 = 53;
const RUNTIME_CONT_SIMD_I8X16_SHUFFLE: u32 = 54;
const RUNTIME_CONT_SIMD_I8X16_SWIZZLE: u32 = 55;
const RUNTIME_CONT_SIMD_V128_LOAD: u32 = 56;
const RUNTIME_CONT_SIMD_V128_LOAD_INDEXED_LOCAL: u32 = 57;
const RUNTIME_CONT_SIMD_V128_LOAD_INDEXED_SHARED: u32 = 58;
const RUNTIME_CONT_SIMD_V128_LOAD_SHARED: u32 = 59;

fn decode_runtime_stub(
    cursor: usize,
    kind: u32,
    pop_slots: usize,
    push_slots: usize,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::RuntimeStub {
        pc_index: cursor,
        kind,
        pop_slots,
        push_slots,
    })
}

fn decode_runtime_continuation_stub(
    _code: &[Instr],
    cursor: usize,
    kind: u32,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::RuntimeContinuationStub {
        pc_index: cursor,
        kind,
    })
}

fn decode_data_drop(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_DATA_DROP, 0, 0)
}

fn decode_elem_drop(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_ELEM_DROP, 0, 0)
}

fn decode_mem_init(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_MEM_INIT_LOCAL, 3, 0)
}

fn decode_mem_init_shared(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_MEM_INIT_SHARED, 3, 0)
}

fn decode_mem_init_indexed_local(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_MEM_INIT_INDEXED_LOCAL, 3, 0)
}

fn decode_mem_init_indexed_shared(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_MEM_INIT_INDEXED_SHARED, 3, 0)
}

fn decode_mem_copy_shared(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_MEM_COPY_SHARED, 3, 0)
}

fn decode_mem_copy_indexed_local_local(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_MEM_COPY_INDEXED_LOCAL_LOCAL, 3, 0)
}

fn decode_mem_copy_indexed_local_shared(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_MEM_COPY_INDEXED_LOCAL_SHARED, 3, 0)
}

fn decode_mem_copy_indexed_shared_local(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_MEM_COPY_INDEXED_SHARED_LOCAL, 3, 0)
}

fn decode_mem_copy_indexed_shared_shared(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_MEM_COPY_INDEXED_SHARED_SHARED, 3, 0)
}

fn decode_mem_fill_shared(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_MEM_FILL_SHARED, 3, 0)
}

fn decode_mem_fill_indexed_local(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_MEM_FILL_INDEXED_LOCAL, 3, 0)
}

fn decode_mem_fill_indexed_shared(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_MEM_FILL_INDEXED_SHARED, 3, 0)
}

fn decode_mem_size_indexed_local(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_MEM_SIZE_INDEXED_LOCAL, 0, 1)
}

fn decode_mem_size_indexed_shared(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_MEM_SIZE_INDEXED_SHARED, 0, 1)
}

fn decode_mem_grow_indexed_local(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_MEM_GROW_INDEXED_LOCAL, 1, 1)
}

fn decode_mem_grow_indexed_shared(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_MEM_GROW_INDEXED_SHARED, 1, 1)
}

fn decode_table_get(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_TABLE_GET, 1, 1)
}

fn decode_table_set(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_TABLE_SET, 2, 0)
}

fn decode_table_init(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_TABLE_INIT, 3, 0)
}

fn decode_table_copy(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_TABLE_COPY, 3, 0)
}

fn decode_table_grow(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_TABLE_GROW, 2, 1)
}

fn decode_table_size(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_TABLE_SIZE, 0, 1)
}

fn decode_table_fill(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_TABLE_FILL, 3, 0)
}

fn decode_call_i32_numeric_token_state_transition(
    _code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_runtime_stub(
        cursor,
        RUNTIME_STUB_CALL_NUMERIC_TOKEN_STATE_TRANSITION,
        2,
        1,
    )
}

fn decode_call_cached_u16_low7_guard(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_CALL_CACHED_U16_LOW7_GUARD, 2, 1)
}

fn decode_i8x16_extract_lane_s(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_I8X16_EXTRACT_LANE_S, 4, 1)
}

fn decode_v128_bitselect(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_V128_BITSELECT, 12, 4)
}

fn decode_atomic_notify_local(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_ATOMIC_NOTIFY_LOCAL, 2, 1)
}

fn decode_atomic_notify_shared(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_ATOMIC_NOTIFY_SHARED, 2, 1)
}

fn decode_atomic_notify_indexed_local(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_ATOMIC_NOTIFY_INDEXED_LOCAL, 2, 1)
}

fn decode_atomic_notify_indexed_shared(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_ATOMIC_NOTIFY_INDEXED_SHARED, 2, 1)
}

fn decode_atomic_wait32_local(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_ATOMIC_WAIT32_LOCAL, 4, 1)
}

fn decode_atomic_wait32_shared(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_ATOMIC_WAIT32_SHARED, 4, 1)
}

fn decode_atomic_wait32_indexed_local(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_ATOMIC_WAIT32_INDEXED_LOCAL, 4, 1)
}

fn decode_atomic_wait32_indexed_shared(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_ATOMIC_WAIT32_INDEXED_SHARED, 4, 1)
}

fn decode_atomic_wait64_local(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_ATOMIC_WAIT64_LOCAL, 5, 1)
}

fn decode_atomic_wait64_shared(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_ATOMIC_WAIT64_SHARED, 5, 1)
}

fn decode_atomic_wait64_indexed_local(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_ATOMIC_WAIT64_INDEXED_LOCAL, 5, 1)
}

fn decode_atomic_wait64_indexed_shared(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_runtime_stub(cursor, RUNTIME_STUB_ATOMIC_WAIT64_INDEXED_SHARED, 5, 1)
}

fn decode_atomic_fence(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::AtomicFence { shared: false })
}

fn decode_atomic_fence_shared(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::AtomicFence { shared: true })
}

fn decode_ref_null(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::RefNull)
}

fn decode_ref_is_null(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::RefIsNull)
}

fn decode_ref_func(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::RefFunc {
        funcidx: operand_u32(code, cursor, 1)?,
    })
}

fn decode_unreachable(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::Trap {
        result: VMResult::Unreachable,
    })
}

fn decode_i32_const(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Const {
        value: operand_u32(code, cursor, 1)?,
    })
}

fn decode_i64_const(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Const {
        value: operand_i64(code, cursor, 1)? as u64,
    })
}

fn decode_f32_const(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F32Const {
        bits: operand_f32_bits(code, cursor, 1)?,
    })
}

fn decode_f64_const(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F64Const {
        bits: operand_f64_bits(code, cursor, 1)?,
    })
}

fn decode_f32_add(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F32Binary {
        op: FloatBinaryOp::Add,
    })
}

fn decode_f32_sub(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F32Binary {
        op: FloatBinaryOp::Sub,
    })
}

fn decode_f32_mul(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F32Binary {
        op: FloatBinaryOp::Mul,
    })
}

fn decode_f32_div(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F32Binary {
        op: FloatBinaryOp::Div,
    })
}

fn decode_f32_min(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F32Binary {
        op: FloatBinaryOp::Min,
    })
}

fn decode_f32_max(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F32Binary {
        op: FloatBinaryOp::Max,
    })
}

fn decode_f32_copysign(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F32Binary {
        op: FloatBinaryOp::Copysign,
    })
}

fn decode_f32_abs(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F32Unary {
        op: FloatUnaryOp::Abs,
    })
}

fn decode_f32_neg(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F32Unary {
        op: FloatUnaryOp::Neg,
    })
}

fn decode_f32_sqrt(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F32Unary {
        op: FloatUnaryOp::Sqrt,
    })
}

fn decode_f32_ceil(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F32Unary {
        op: FloatUnaryOp::Ceil,
    })
}

fn decode_f32_floor(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F32Unary {
        op: FloatUnaryOp::Floor,
    })
}

fn decode_f32_trunc(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F32Unary {
        op: FloatUnaryOp::Trunc,
    })
}

fn decode_f32_nearest(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F32Unary {
        op: FloatUnaryOp::Nearest,
    })
}

fn decode_f32_convert_i32_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F32ConvertI32 { signed: true })
}

fn decode_f32_convert_i32_u(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F32ConvertI32 { signed: false })
}

fn decode_f32_convert_i64_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F32ConvertI64 { signed: true })
}

fn decode_f32_convert_i64_u(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F32ConvertI64 { signed: false })
}

fn decode_f32_demote_f64(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F32DemoteF64)
}

fn decode_f64_add(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F64Binary {
        op: FloatBinaryOp::Add,
    })
}

fn decode_f64_sub(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F64Binary {
        op: FloatBinaryOp::Sub,
    })
}

fn decode_f64_mul(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F64Binary {
        op: FloatBinaryOp::Mul,
    })
}

fn decode_f64_div(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F64Binary {
        op: FloatBinaryOp::Div,
    })
}

fn decode_f64_min(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F64Binary {
        op: FloatBinaryOp::Min,
    })
}

fn decode_f64_max(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F64Binary {
        op: FloatBinaryOp::Max,
    })
}

fn decode_f64_copysign(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F64Binary {
        op: FloatBinaryOp::Copysign,
    })
}

fn decode_f64_neg(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F64Unary {
        op: FloatUnaryOp::Neg,
    })
}

fn decode_f64_abs(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F64Unary {
        op: FloatUnaryOp::Abs,
    })
}

fn decode_f64_sqrt(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F64Unary {
        op: FloatUnaryOp::Sqrt,
    })
}

fn decode_f64_ceil(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F64Unary {
        op: FloatUnaryOp::Ceil,
    })
}

fn decode_f64_floor(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F64Unary {
        op: FloatUnaryOp::Floor,
    })
}

fn decode_f64_trunc(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F64Unary {
        op: FloatUnaryOp::Trunc,
    })
}

fn decode_f64_nearest(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F64Unary {
        op: FloatUnaryOp::Nearest,
    })
}

fn decode_f64_convert_i32_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F64ConvertI32 { signed: true })
}

fn decode_f64_convert_i32_u(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F64ConvertI32 { signed: false })
}

fn decode_f64_convert_i64_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F64ConvertI64 { signed: true })
}

fn decode_f64_convert_i64_u(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F64ConvertI64 { signed: false })
}

fn decode_f64_promote_f32(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F64PromoteF32)
}

fn decode_i64_extend_i32_u(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64ExtendI32 { signed: false })
}

fn decode_i64_extend_i32_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64ExtendI32 { signed: true })
}

fn decode_i64_extend8_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64ExtendS { bits: 8 })
}

fn decode_i64_extend16_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64ExtendS { bits: 16 })
}

fn decode_i64_extend32_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64ExtendS { bits: 32 })
}

fn decode_i64_eqz(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Eqz)
}

fn decode_i64_clz(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Unary {
        op: I64UnaryOp::Clz,
    })
}

fn decode_i64_ctz(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Unary {
        op: I64UnaryOp::Ctz,
    })
}

fn decode_i64_popcnt(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Unary {
        op: I64UnaryOp::Popcnt,
    })
}

fn decode_i64_eq(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Compare {
        op: I64CompareOp::Eq,
    })
}

fn decode_i64_ne(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Compare {
        op: I64CompareOp::Ne,
    })
}

fn decode_i64_lt_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Compare {
        op: I64CompareOp::LtS,
    })
}

fn decode_i64_lt_u(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Compare {
        op: I64CompareOp::LtU,
    })
}

fn decode_i64_gt_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Compare {
        op: I64CompareOp::GtS,
    })
}

fn decode_i64_gt_u(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Compare {
        op: I64CompareOp::GtU,
    })
}

fn decode_i64_le_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Compare {
        op: I64CompareOp::LeS,
    })
}

fn decode_i64_le_u(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Compare {
        op: I64CompareOp::LeU,
    })
}

fn decode_i64_ge_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Compare {
        op: I64CompareOp::GeS,
    })
}

fn decode_i64_ge_u(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Compare {
        op: I64CompareOp::GeU,
    })
}

fn decode_i64_add(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Binary {
        op: I64BinaryOp::Add,
    })
}

fn decode_i64_mul(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Binary {
        op: I64BinaryOp::Mul,
    })
}

fn decode_i64_div_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Binary {
        op: I64BinaryOp::DivS,
    })
}

fn decode_i64_div_u(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Binary {
        op: I64BinaryOp::DivU,
    })
}

fn decode_i64_rem_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Binary {
        op: I64BinaryOp::RemS,
    })
}

fn decode_i64_rem_u(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Binary {
        op: I64BinaryOp::RemU,
    })
}

fn decode_i64_sub(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Binary {
        op: I64BinaryOp::Sub,
    })
}

fn decode_i64_and(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Binary {
        op: I64BinaryOp::And,
    })
}

fn decode_i64_or(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Binary {
        op: I64BinaryOp::Or,
    })
}

fn decode_i64_xor(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Binary {
        op: I64BinaryOp::Xor,
    })
}

fn decode_i64_shl(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Binary {
        op: I64BinaryOp::Shl,
    })
}

fn decode_i64_shr_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Binary {
        op: I64BinaryOp::ShrS,
    })
}

fn decode_i64_shr_u(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Binary {
        op: I64BinaryOp::ShrU,
    })
}

fn decode_i64_rotl(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Binary {
        op: I64BinaryOp::Rotl,
    })
}

fn decode_i64_rotr(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Binary {
        op: I64BinaryOp::Rotr,
    })
}

fn decode_i32_wrap_i64(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32WrapI64)
}

fn decode_i32_const_set4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32ConstWrite4 {
        value: operand_u32(code, cursor, 1)?,
        local: operand_local(code, cursor, 2)?,
        keep_result: false,
    })
}

fn decode_i32_const_tee4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32ConstWrite4 {
        value: operand_u32(code, cursor, 1)?,
        local: operand_local(code, cursor, 2)?,
        keep_result: true,
    })
}

fn decode_i32_const_binop(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32ConstBinop {
        kind: operand_u32(code, cursor, 1)?,
        rhs: operand_i32(code, cursor, 2)? as u32,
    })
}

fn decode_i32_const_binop_br_if(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32ConstBinopBrIf {
        kind: operand_u32(code, cursor, 1)?,
        rhs: operand_i32(code, cursor, 2)? as u32,
        target: operand_jump_addr(code, cursor, 3)?,
    })
}

fn decode_i32_const_binop_set4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_const_binop_write4(code, cursor, false)
}

fn decode_i32_const_binop_tee4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_const_binop_write4(code, cursor, true)
}

fn decode_i32_const_binop_write4(
    code: &[Instr],
    cursor: usize,
    keep_result: bool,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32ConstBinopWrite4 {
        kind: operand_u32(code, cursor, 1)?,
        rhs: operand_i32(code, cursor, 2)? as u32,
        dst: operand_local(code, cursor, 3)?,
        keep_result,
    })
}

fn decode_i32_const_cmp(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32ConstCmpWrite4 {
        kind: operand_u32(code, cursor, 1)?,
        rhs: operand_i32(code, cursor, 2)? as u32,
        dst: u32::MAX,
        keep_result: true,
    })
}

fn decode_i32_const_cmp_br_if(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32ConstCmpBrIf {
        kind: operand_u32(code, cursor, 1)?,
        rhs: operand_i32(code, cursor, 2)? as u32,
        target: operand_jump_addr(code, cursor, 3)?,
    })
}

fn decode_i32_const_cmp_set4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_const_cmp_write4(code, cursor, false)
}

fn decode_i32_const_cmp_tee4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_const_cmp_write4(code, cursor, true)
}

fn decode_i32_const_cmp_write4(
    code: &[Instr],
    cursor: usize,
    keep_result: bool,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32ConstCmpWrite4 {
        kind: operand_u32(code, cursor, 1)?,
        rhs: operand_i32(code, cursor, 2)? as u32,
        dst: operand_local(code, cursor, 3)?,
        keep_result,
    })
}

fn decode_local_binop32(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalBinop32Write4 {
        kind: operand_u32(code, cursor, 1)?,
        lhs: operand_local(code, cursor, 2)?,
        rhs: operand_u32(code, cursor, 3)?,
        dst: u32::MAX,
        keep_result: true,
    })
}

fn decode_local_binop32_br_if(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalBinop32BrIf {
        kind: operand_u32(code, cursor, 1)?,
        lhs: operand_local(code, cursor, 2)?,
        rhs: operand_u32(code, cursor, 3)?,
        target: operand_jump_addr(code, cursor, 4)?,
    })
}

fn decode_local_cmp32(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalCmp32Write4 {
        kind: operand_u32(code, cursor, 1)?,
        lhs: operand_local(code, cursor, 2)?,
        rhs: operand_u32(code, cursor, 3)?,
        dst: u32::MAX,
        keep_result: true,
    })
}

fn decode_local_cmp32_br_if(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalCmp32BrIf {
        kind: operand_u32(code, cursor, 1)?,
        lhs: operand_local(code, cursor, 2)?,
        rhs: operand_u32(code, cursor, 3)?,
        target: operand_jump_addr(code, cursor, 4)?,
    })
}

fn decode_local_cmp32_set4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_local_cmp32_write4(code, cursor, false)
}

fn decode_local_cmp32_tee4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_local_cmp32_write4(code, cursor, true)
}

fn decode_local_cmp32_write4(
    code: &[Instr],
    cursor: usize,
    keep_result: bool,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalCmp32Write4 {
        kind: operand_u32(code, cursor, 1)?,
        lhs: operand_local(code, cursor, 2)?,
        rhs: operand_u32(code, cursor, 3)?,
        dst: operand_local(code, cursor, 4)?,
        keep_result,
    })
}

fn decode_local_cmp64(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_local_cmp64_write4(code, cursor, u32::MAX, true)
}

fn decode_local_cmp64_set4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    let dst = operand_local(code, cursor, 4)?;
    decode_local_cmp64_write4(code, cursor, dst, false)
}

fn decode_local_cmp64_tee4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    let dst = operand_local(code, cursor, 4)?;
    decode_local_cmp64_write4(code, cursor, dst, true)
}

fn decode_local_cmp64_write4(
    code: &[Instr],
    cursor: usize,
    dst: u32,
    keep_result: bool,
) -> Result<BaselineOp, ()> {
    let kind = operand_u32(code, cursor, 1)?;
    let (_, rhs_shape) = crate::common::decode_local_cmp64_kind(kind).ok_or(())?;
    let rhs = match rhs_shape {
        LocalFastRhsShape::Local => u64::from(operand_local(code, cursor, 3)?),
        LocalFastRhsShape::Const => operand_i64(code, cursor, 3)? as u64,
    };
    Ok(BaselineOp::LocalCmp64Write4 {
        kind,
        lhs: operand_local(code, cursor, 2)?,
        rhs,
        dst,
        keep_result,
    })
}

fn decode_local_cmp64_br_if(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    let kind = operand_u32(code, cursor, 1)?;
    let (_, rhs_shape) = crate::common::decode_local_cmp64_kind(kind).ok_or(())?;
    let rhs = match rhs_shape {
        LocalFastRhsShape::Local => u64::from(operand_local(code, cursor, 3)?),
        LocalFastRhsShape::Const => operand_i64(code, cursor, 3)? as u64,
    };
    Ok(BaselineOp::LocalCmp64BrIf {
        kind,
        lhs: operand_local(code, cursor, 2)?,
        rhs,
        target: operand_jump_addr(code, cursor, 4)?,
    })
}

fn decode_local_unary32(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalUnary32Write4 {
        kind: operand_u32(code, cursor, 1)?,
        src: operand_local(code, cursor, 2)?,
        dst: u32::MAX,
        keep_result: true,
    })
}

fn decode_local_unary32_set4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_local_unary32_write4(code, cursor, false)
}

fn decode_local_unary32_tee4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_local_unary32_write4(code, cursor, true)
}

fn decode_local_unary32_write4(
    code: &[Instr],
    cursor: usize,
    keep_result: bool,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalUnary32Write4 {
        kind: operand_u32(code, cursor, 1)?,
        src: operand_local(code, cursor, 2)?,
        dst: operand_local(code, cursor, 3)?,
        keep_result,
    })
}

fn decode_local_unary64(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalUnary64Write8 {
        kind: operand_u32(code, cursor, 1)?,
        src: operand_local(code, cursor, 2)?,
        dst: u32::MAX,
        keep_result: true,
    })
}

fn decode_local_unary64_set8(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_local_unary64_write8(code, cursor, false)
}

fn decode_local_unary64_tee8(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_local_unary64_write8(code, cursor, true)
}

fn decode_local_unary64_write8(
    code: &[Instr],
    cursor: usize,
    keep_result: bool,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalUnary64Write8 {
        kind: operand_u32(code, cursor, 1)?,
        src: operand_local(code, cursor, 2)?,
        dst: operand_local(code, cursor, 3)?,
        keep_result,
    })
}

fn decode_local_binop32_set4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_local_binop32_write4(code, cursor, false)
}

fn decode_local_binop32_tee4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_local_binop32_write4(code, cursor, true)
}

fn decode_local_binop64(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    let kind = operand_u32(code, cursor, 1)?;
    let (_, rhs_shape) = decode_local_binop64_kind(kind).ok_or(())?;
    let rhs = match rhs_shape {
        LocalFastRhsShape::Local => u64::from(operand_local(code, cursor, 3)?),
        LocalFastRhsShape::Const => operand_i64(code, cursor, 3)? as u64,
    };
    Ok(BaselineOp::LocalBinop64 {
        kind,
        lhs: operand_local(code, cursor, 2)?,
        rhs,
    })
}

fn decode_local_binop64_set8(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_local_binop64_write8(code, cursor, false)
}

fn decode_local_binop64_tee8(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_local_binop64_write8(code, cursor, true)
}

fn decode_local_binop64_write8(
    code: &[Instr],
    cursor: usize,
    keep_result: bool,
) -> Result<BaselineOp, ()> {
    let kind = operand_u32(code, cursor, 1)?;
    let (_, rhs_shape) = decode_local_binop64_kind(kind).ok_or(())?;
    let rhs = match rhs_shape {
        LocalFastRhsShape::Local => u64::from(operand_local(code, cursor, 3)?),
        LocalFastRhsShape::Const => operand_i64(code, cursor, 3)? as u64,
    };
    Ok(BaselineOp::LocalBinop64Write8 {
        kind,
        lhs: operand_local(code, cursor, 2)?,
        rhs,
        dst: operand_local(code, cursor, 4)?,
        keep_result,
    })
}

fn decode_local_binop32_write4(
    code: &[Instr],
    cursor: usize,
    keep_result: bool,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalBinop32Write4 {
        kind: operand_u32(code, cursor, 1)?,
        lhs: operand_local(code, cursor, 2)?,
        rhs: operand_u32(code, cursor, 3)?,
        dst: operand_local(code, cursor, 4)?,
        keep_result,
    })
}

fn decode_local_get4_i32_const_add(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalGet4I32ConstAdd {
        local: operand_local(code, cursor, 1)?,
        value: operand_u32(code, cursor, 2)?,
    })
}

fn decode_local_get4_i32_const_add_set4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_local_get4_i32_const_add_write4(code, cursor, false)
}

fn decode_local_get4_i32_const_add_tee4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_local_get4_i32_const_add_write4(code, cursor, true)
}

fn decode_local_get4_i32_const_add_write4(
    code: &[Instr],
    cursor: usize,
    keep_result: bool,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalGet4I32ConstAddWrite4 {
        src: operand_local(code, cursor, 1)?,
        value: operand_u32(code, cursor, 2)?,
        dst: operand_local(code, cursor, 3)?,
        keep_result,
    })
}

fn decode_local_get4_local_get4_i32_add(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalGet4LocalGet4I32Add {
        lhs: operand_local(code, cursor, 1)?,
        rhs: operand_local(code, cursor, 2)?,
    })
}

fn decode_local_get4_local_get4_i32_add_set4(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_local_get4_local_get4_i32_add_write4(code, cursor, false)
}

fn decode_local_get4_local_get4_i32_add_tee4(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_local_get4_local_get4_i32_add_write4(code, cursor, true)
}

fn decode_local_get4_local_get4_i32_add_write4(
    code: &[Instr],
    cursor: usize,
    keep_result: bool,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalGet4LocalGet4I32AddWrite4 {
        lhs: operand_local(code, cursor, 1)?,
        rhs: operand_local(code, cursor, 2)?,
        dst: operand_local(code, cursor, 3)?,
        keep_result,
    })
}

fn decode_local_get4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    if local_get4_local_get4_const_shr_u_tee4_eq_matches(code, cursor) {
        return Ok(BaselineOp::LocalGet4LocalGet4ConstShrUTee4Eq {
            first: operand_local(code, cursor, 1)?,
            second: operand_local(code, cursor + 2, 1)?,
            shift: operand_u32(code, cursor + 4, 1)? & 31,
            dst: operand_local(code, cursor + 7, 1)?,
        });
    }
    if let (Ok(second), Ok(compare)) = (instr(code, cursor + 2), instr(code, cursor + 4)) {
        let second_is_local_get4 = std::ptr::fn_addr_eq(unsafe { second.op }, op_local_get4 as Op);
        if second_is_local_get4 {
            if let Some(op) = i32_compare_op_from_op(unsafe { compare.op }) {
                return Ok(BaselineOp::LocalGet4LocalGet4Compare {
                    first: operand_local(code, cursor, 1)?,
                    second: operand_local(code, cursor + 2, 1)?,
                    op,
                });
            }
        }
    }
    Ok(BaselineOp::LocalGet4 {
        local: operand_local(code, cursor, 1)?,
    })
}

fn local_get4_local_get4_const_shr_u_tee4_eq_matches(code: &[Instr], cursor: usize) -> bool {
    let ops = [
        (cursor + 2, op_local_get4 as Op),
        (cursor + 4, op_i32_const as Op),
        (cursor + 6, op_i32_shr_u as Op),
        (cursor + 7, op_local_tee4 as Op),
        (cursor + 9, op_i32_eq as Op),
    ];
    ops.into_iter().all(|(pc, expected)| {
        instr(code, pc)
            .map(|instr| std::ptr::fn_addr_eq(unsafe { instr.op }, expected))
            .unwrap_or(false)
    })
}

fn decode_local_get4_run(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    let count = operand_u32(code, cursor, 1)? as usize;
    if !(4..=16).contains(&count) {
        return Err(());
    }
    let mut locals = [0u32; 16];
    for (index, slot) in locals.iter_mut().take(count).enumerate() {
        *slot = operand_local(code, cursor, 2 + index)?;
    }
    Ok(BaselineOp::LocalGet4Run { locals, count })
}

fn decode_local_get4_run_skip(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    let count = operand_u32(code, cursor, 1)? as usize;
    if !(4..=16).contains(&count) {
        return Err(());
    }
    let mut locals = [0u32; 16];
    for (index, slot) in locals.iter_mut().take(count).enumerate() {
        *slot = operand_local(code, cursor, 2 + index)?;
    }
    Ok(BaselineOp::LocalGet4RunSkip {
        locals,
        count,
        skip_slots: operand_u32(code, cursor, 2 + count)? as usize,
    })
}

fn decode_local_get8(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalGet8 {
        local: operand_local(code, cursor, 1)?,
    })
}

fn decode_local_get16(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalGet16 {
        local: operand_local(code, cursor, 1)?,
    })
}

fn decode_global_get4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::GlobalGet4 {
        index: operand_u32(code, cursor, 1)?,
    })
}

fn decode_global_get8(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::GlobalGetSlots {
        index: operand_u32(code, cursor, 1)?,
        slots: 2,
    })
}

fn decode_global_get16(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::GlobalGetSlots {
        index: operand_u32(code, cursor, 1)?,
        slots: 4,
    })
}

fn decode_global_set4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::GlobalSet4 {
        index: operand_u32(code, cursor, 1)?,
    })
}

fn decode_global_set8(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::GlobalSetSlots {
        index: operand_u32(code, cursor, 1)?,
        slots: 2,
    })
}

fn decode_global_set16(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::GlobalSetSlots {
        index: operand_u32(code, cursor, 1)?,
        slots: 4,
    })
}

fn decode_drop(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::Drop {
        size: unsafe { instr(code, cursor + 1)?.operand.drop_size },
    })
}

fn decode_local_get4_local_get4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalGet4LocalGet4 {
        first: operand_local(code, cursor, 1)?,
        second: operand_local(code, cursor, 2)?,
    })
}

fn decode_local_get4_local_get4_local_get4(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalGet4LocalGet4LocalGet4 {
        first: operand_local(code, cursor, 1)?,
        second: operand_local(code, cursor, 2)?,
        third: operand_local(code, cursor, 3)?,
    })
}

fn decode_local_get4_local_get4_xor_tee4_load16_u(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalGet4LocalGet4XorTee4Load16U {
        lhs: operand_local(code, cursor, 1)?,
        rhs: operand_local(code, cursor, 2)?,
        dst: operand_local(code, cursor, 3)?,
        memarg: operand_memarg(code, cursor, 4)?,
    })
}

fn decode_local_get4_i32_load_local_base(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_local_get4_i32_load_local_base_width(code, cursor, 4, false)
}

fn decode_local_get4_i32_load8_u_local_base(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_local_get4_i32_load_local_base_width(code, cursor, 1, false)
}

fn decode_local_get4_i32_load8_s_local_base(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_local_get4_i32_load_local_base_width(code, cursor, 1, true)
}

fn decode_local_get4_i32_load16_u_local_base(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_local_get4_i32_load_local_base_width(code, cursor, 2, false)
}

fn decode_local_get4_i32_load16_s_local_base(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_local_get4_i32_load_local_base_width(code, cursor, 2, true)
}

fn decode_local_get4_i32_load_local_base_width(
    code: &[Instr],
    cursor: usize,
    width: u32,
    signed: bool,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalGet4I32LoadLocalBase {
        preserved: operand_local(code, cursor, 1)?,
        base_local: operand_local(code, cursor, 2)?,
        delta: operand_i32(code, cursor, 3)? as u32,
        memarg: operand_memarg(code, cursor, 4)?,
        width,
        signed,
    })
}

fn decode_local_get4_i32_inc_local_base(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalGet4I32IncLocalBase {
        preserved: operand_local(code, cursor, 1)?,
        base_local: operand_local(code, cursor, 2)?,
        store_delta: operand_i32(code, cursor, 3)? as u32,
        load_delta: operand_i32(code, cursor, 4)? as u32,
        load_memarg: operand_memarg(code, cursor, 5)?,
        store_memarg: operand_memarg(code, cursor, 6)?,
    })
}

fn decode_local_get4_i32_load8_u_local_base_set4(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalGet4I32Load8ULocalBaseSet4 {
        preserved: operand_local(code, cursor, 1)?,
        load_base_local: operand_local(code, cursor, 2)?,
        load_delta: operand_i32(code, cursor, 3)? as u32,
        load_memarg: operand_memarg(code, cursor, 4)?,
        dst: operand_local(code, cursor, 5)?,
    })
}

fn decode_local_get4_i32_inc_local_base_i32_load8_u_local_base_set4(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalGet4I32IncLocalBaseI32Load8ULocalBaseSet4 {
        preserved: operand_local(code, cursor, 1)?,
        inc_base_local: operand_local(code, cursor, 2)?,
        inc_store_delta: operand_i32(code, cursor, 3)? as u32,
        inc_load_delta: operand_i32(code, cursor, 4)? as u32,
        inc_load_memarg: operand_memarg(code, cursor, 5)?,
        inc_store_memarg: operand_memarg(code, cursor, 6)?,
        load_base_local: operand_local(code, cursor, 7)?,
        load_delta: operand_i32(code, cursor, 8)? as u32,
        load_memarg: operand_memarg(code, cursor, 9)?,
        dst: operand_local(code, cursor, 10)?,
    })
}

fn decode_local_get4_i32_load_local_base_i32_add_set4(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_local_get4_i32_load_local_base_i32_add_write4(code, cursor, false)
}

fn decode_local_get4_i32_load_local_base_i32_add_tee4(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_local_get4_i32_load_local_base_i32_add_write4(code, cursor, true)
}

fn decode_local_get4_i32_load_local_base_i32_add_write4(
    code: &[Instr],
    cursor: usize,
    keep_result: bool,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalGet4I32LoadLocalBaseI32AddWrite4 {
        rhs: operand_local(code, cursor, 1)?,
        base_local: operand_local(code, cursor, 2)?,
        delta: operand_i32(code, cursor, 3)? as u32,
        memarg: operand_memarg(code, cursor, 4)?,
        dst: operand_local(code, cursor, 5)?,
        keep_result,
    })
}

fn decode_local_get4x3_i32_add_const_binop_i32_add_set4(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_local_get4x3_i32_add_const_binop_i32_add_write4(code, cursor, false)
}

fn decode_local_get4x3_i32_add_const_binop_i32_add_tee4(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_local_get4x3_i32_add_const_binop_i32_add_write4(code, cursor, true)
}

fn decode_local_get4x3_i32_add_const_binop_i32_add_write4(
    code: &[Instr],
    cursor: usize,
    keep_result: bool,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalGet4x3I32AddConstBinopI32AddWrite4 {
        first: operand_local(code, cursor, 1)?,
        second: operand_local(code, cursor, 2)?,
        third: operand_local(code, cursor, 3)?,
        kind: operand_u32(code, cursor, 4)?,
        rhs: operand_i32(code, cursor, 5)? as u32,
        dst: operand_local(code, cursor, 6)?,
        skip_slots: operand_u32(code, cursor, 7)? as usize,
        keep_result,
    })
}

fn decode_local_get4x3_i32_add_const_binop_i32_add_tee4_i32_const_store(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    Ok(
        BaselineOp::LocalGet4x3I32AddConstBinopI32AddTee4I32ConstStore {
            first: operand_local(code, cursor, 1)?,
            second: operand_local(code, cursor, 2)?,
            third: operand_local(code, cursor, 3)?,
            kind: operand_u32(code, cursor, 4)?,
            rhs: operand_i32(code, cursor, 5)? as u32,
            dst: operand_local(code, cursor, 6)?,
            value: operand_i32(code, cursor, 7)? as u32,
            memarg: operand_memarg(code, cursor, 8)?,
            skip_slots: operand_u32(code, cursor, 9)? as usize,
        },
    )
}

fn decode_local_get4_set4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_local_get4_write4(code, cursor, false)
}

fn decode_local_get4_tee4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_local_get4_write4(code, cursor, true)
}

fn decode_local_get4_write4(
    code: &[Instr],
    cursor: usize,
    keep_result: bool,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalGet4Write4 {
        src: operand_local(code, cursor, 1)?,
        dst: operand_local(code, cursor, 2)?,
        keep_result,
    })
}

fn decode_local_set4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalSet4 {
        local: operand_local(code, cursor, 1)?,
    })
}

fn decode_local_set8(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalSet8 {
        local: operand_local(code, cursor, 1)?,
    })
}

fn decode_local_set16(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalSet16 {
        local: operand_local(code, cursor, 1)?,
    })
}

fn decode_local_tee4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalTee4 {
        local: operand_local(code, cursor, 1)?,
    })
}

fn decode_local_tee8(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalTee8 {
        local: operand_local(code, cursor, 1)?,
    })
}

fn decode_local_tee16(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalTee16 {
        local: operand_local(code, cursor, 1)?,
    })
}

fn decode_select4(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::Select4 {
        dst: None,
        keep_result: true,
    })
}

fn decode_select8(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::SelectSlots { slots: 2 })
}

fn decode_select16(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::SelectSlots { slots: 4 })
}

fn decode_select(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    match unsafe { instr(code, cursor + 1)?.operand.select } {
        4 => decode_select4(code, cursor),
        8 => decode_select8(code, cursor),
        16 => decode_select16(code, cursor),
        _ => Err(()),
    }
}

fn decode_select4_set4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::Select4 {
        dst: Some(operand_local(code, cursor, 1)?),
        keep_result: false,
    })
}

fn decode_select4_tee4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::Select4 {
        dst: Some(operand_local(code, cursor, 1)?),
        keep_result: true,
    })
}

fn decode_i32_add(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Binary {
        op: I32BinaryOp::Add,
    })
}

fn decode_i32_sub(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Binary {
        op: I32BinaryOp::Sub,
    })
}

fn decode_i32_mul(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Binary {
        op: I32BinaryOp::Mul,
    })
}

fn decode_i32_div_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Binary {
        op: I32BinaryOp::DivS,
    })
}

fn decode_i32_div_u(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Binary {
        op: I32BinaryOp::DivU,
    })
}

fn decode_i32_rem_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Binary {
        op: I32BinaryOp::RemS,
    })
}

fn decode_i32_rem_u(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Binary {
        op: I32BinaryOp::RemU,
    })
}

fn decode_i32_and(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Binary {
        op: I32BinaryOp::And,
    })
}

fn decode_i32_or(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Binary {
        op: I32BinaryOp::Or,
    })
}

fn decode_i32_xor(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Binary {
        op: I32BinaryOp::Xor,
    })
}

fn decode_i32_shl(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Binary {
        op: I32BinaryOp::Shl,
    })
}

fn decode_i32_shr_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Binary {
        op: I32BinaryOp::ShrS,
    })
}

fn decode_i32_shr_u(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Binary {
        op: I32BinaryOp::ShrU,
    })
}

fn decode_i32_rotl(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Binary {
        op: I32BinaryOp::Rotl,
    })
}

fn decode_i32_rotr(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Binary {
        op: I32BinaryOp::Rotr,
    })
}

fn decode_i32_clz(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Unary {
        op: I32UnaryOp::Clz,
    })
}

fn decode_i32_ctz(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Unary {
        op: I32UnaryOp::Ctz,
    })
}

fn decode_i32_popcnt(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Unary {
        op: I32UnaryOp::Popcnt,
    })
}

fn decode_i32_extend8_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Unary {
        op: I32UnaryOp::Extend8S,
    })
}

fn decode_i32_extend16_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Unary {
        op: I32UnaryOp::Extend16S,
    })
}

fn decode_i32_eqz(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Eqz)
}

fn decode_i32_eq(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Compare {
        op: I32CompareOp::Eq,
    })
}

fn i32_compare_op_from_op(op: Op) -> Option<I32CompareOp> {
    if std::ptr::fn_addr_eq(op, op_i32_eq as Op) {
        Some(I32CompareOp::Eq)
    } else if std::ptr::fn_addr_eq(op, op_i32_ne as Op) {
        Some(I32CompareOp::Ne)
    } else if std::ptr::fn_addr_eq(op, op_i32_lt_s as Op) {
        Some(I32CompareOp::LtS)
    } else if std::ptr::fn_addr_eq(op, op_i32_lt_u as Op) {
        Some(I32CompareOp::LtU)
    } else if std::ptr::fn_addr_eq(op, op_i32_gt_s as Op) {
        Some(I32CompareOp::GtS)
    } else if std::ptr::fn_addr_eq(op, op_i32_gt_u as Op) {
        Some(I32CompareOp::GtU)
    } else if std::ptr::fn_addr_eq(op, op_i32_le_s as Op) {
        Some(I32CompareOp::LeS)
    } else if std::ptr::fn_addr_eq(op, op_i32_le_u as Op) {
        Some(I32CompareOp::LeU)
    } else if std::ptr::fn_addr_eq(op, op_i32_ge_s as Op) {
        Some(I32CompareOp::GeS)
    } else if std::ptr::fn_addr_eq(op, op_i32_ge_u as Op) {
        Some(I32CompareOp::GeU)
    } else {
        None
    }
}

fn decode_i32_ne(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Compare {
        op: I32CompareOp::Ne,
    })
}

fn decode_i32_lt_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Compare {
        op: I32CompareOp::LtS,
    })
}

fn decode_i32_lt_u(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Compare {
        op: I32CompareOp::LtU,
    })
}

fn decode_i32_gt_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Compare {
        op: I32CompareOp::GtS,
    })
}

fn decode_i32_gt_u(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Compare {
        op: I32CompareOp::GtU,
    })
}

fn decode_i32_le_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Compare {
        op: I32CompareOp::LeS,
    })
}

fn decode_i32_le_u(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Compare {
        op: I32CompareOp::LeU,
    })
}

fn decode_i32_ge_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Compare {
        op: I32CompareOp::GeS,
    })
}

fn decode_i32_ge_u(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Compare {
        op: I32CompareOp::GeU,
    })
}

fn decode_f32_eq(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F32Compare {
        op: FloatCompareOp::Eq,
    })
}

fn decode_f32_ne(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F32Compare {
        op: FloatCompareOp::Ne,
    })
}

fn decode_f32_lt(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F32Compare {
        op: FloatCompareOp::Lt,
    })
}

fn decode_f32_gt(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F32Compare {
        op: FloatCompareOp::Gt,
    })
}

fn decode_f32_le(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F32Compare {
        op: FloatCompareOp::Le,
    })
}

fn decode_f32_ge(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F32Compare {
        op: FloatCompareOp::Ge,
    })
}

fn decode_f64_eq(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F64Compare {
        op: FloatCompareOp::Eq,
    })
}

fn decode_f64_ne(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F64Compare {
        op: FloatCompareOp::Ne,
    })
}

fn decode_f64_lt(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F64Compare {
        op: FloatCompareOp::Lt,
    })
}

fn decode_f64_gt(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F64Compare {
        op: FloatCompareOp::Gt,
    })
}

fn decode_f64_le(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F64Compare {
        op: FloatCompareOp::Le,
    })
}

fn decode_f64_ge(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F64Compare {
        op: FloatCompareOp::Ge,
    })
}

fn decode_i32_trunc_f32_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32TruncFloat {
        source: FloatWidth::F32,
        signed: true,
    })
}

fn decode_i32_trunc_f32_u(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32TruncFloat {
        source: FloatWidth::F32,
        signed: false,
    })
}

fn decode_i32_trunc_f64_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32TruncFloat {
        source: FloatWidth::F64,
        signed: true,
    })
}

fn decode_i32_trunc_f64_u(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32TruncFloat {
        source: FloatWidth::F64,
        signed: false,
    })
}

fn decode_i32_trunc_sat_f32_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32TruncSatFloat {
        source: FloatWidth::F32,
        signed: true,
    })
}

fn decode_i32_trunc_sat_f32_u(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32TruncSatFloat {
        source: FloatWidth::F32,
        signed: false,
    })
}

fn decode_i32_trunc_sat_f64_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32TruncSatFloat {
        source: FloatWidth::F64,
        signed: true,
    })
}

fn decode_i32_trunc_sat_f64_u(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32TruncSatFloat {
        source: FloatWidth::F64,
        signed: false,
    })
}

fn decode_i64_trunc_f32_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64TruncFloat {
        source: FloatWidth::F32,
        signed: true,
        saturating: false,
    })
}

fn decode_i64_trunc_f32_u(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64TruncFloat {
        source: FloatWidth::F32,
        signed: false,
        saturating: false,
    })
}

fn decode_i64_trunc_f64_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64TruncFloat {
        source: FloatWidth::F64,
        signed: true,
        saturating: false,
    })
}

fn decode_i64_trunc_f64_u(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64TruncFloat {
        source: FloatWidth::F64,
        signed: false,
        saturating: false,
    })
}

fn decode_i64_trunc_sat_f32_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64TruncFloat {
        source: FloatWidth::F32,
        signed: true,
        saturating: true,
    })
}

fn decode_i64_trunc_sat_f32_u(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64TruncFloat {
        source: FloatWidth::F32,
        signed: false,
        saturating: true,
    })
}

fn decode_i64_trunc_sat_f64_s(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64TruncFloat {
        source: FloatWidth::F64,
        signed: true,
        saturating: true,
    })
}

fn decode_i64_trunc_sat_f64_u(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64TruncFloat {
        source: FloatWidth::F64,
        signed: false,
        saturating: true,
    })
}

fn decode_i32_load(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_width(code, cursor, 4, false)
}

fn decode_i32_load8_u(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_width(code, cursor, 1, false)
}

fn decode_i32_load8_s(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_width(code, cursor, 1, true)
}

fn decode_i32_load16_u(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_width(code, cursor, 2, false)
}

fn decode_i32_load16_s(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_width(code, cursor, 2, true)
}

fn decode_i32_load_local_get4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_local_get4_width(code, cursor, 4, false)
}

fn decode_i32_load8_u_local_get4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_local_get4_width(code, cursor, 1, false)
}

fn decode_i32_load8_s_local_get4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_local_get4_width(code, cursor, 1, true)
}

fn decode_i32_load16_u_local_get4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_local_get4_width(code, cursor, 2, false)
}

fn decode_i32_load16_s_local_get4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_local_get4_width(code, cursor, 2, true)
}

fn decode_i32_load_width(
    code: &[Instr],
    cursor: usize,
    width: u32,
    signed: bool,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Load {
        memarg: operand_memarg(code, cursor, 1)?,
        width,
        signed,
    })
}

fn decode_i32_load_local_get4_width(
    code: &[Instr],
    cursor: usize,
    width: u32,
    signed: bool,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32LoadLocalGet4 {
        memarg: operand_memarg(code, cursor, 1)?,
        width,
        signed,
        local: operand_local(code, cursor, 2)?,
    })
}

fn decode_i64_load(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Load {
        memarg: operand_memarg(code, cursor, 1)?,
        width: 8,
        signed: false,
    })
}

fn decode_i64_load8_s(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Load {
        memarg: operand_memarg(code, cursor, 1)?,
        width: 1,
        signed: true,
    })
}

fn decode_i64_load8_u(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Load {
        memarg: operand_memarg(code, cursor, 1)?,
        width: 1,
        signed: false,
    })
}

fn decode_i64_load16_s(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Load {
        memarg: operand_memarg(code, cursor, 1)?,
        width: 2,
        signed: true,
    })
}

fn decode_i64_load16_u(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Load {
        memarg: operand_memarg(code, cursor, 1)?,
        width: 2,
        signed: false,
    })
}

fn decode_i64_load32_s(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Load {
        memarg: operand_memarg(code, cursor, 1)?,
        width: 4,
        signed: true,
    })
}

fn decode_i64_load32_u(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Load {
        memarg: operand_memarg(code, cursor, 1)?,
        width: 4,
        signed: false,
    })
}

fn decode_f32_load(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_width(code, cursor, 4, false)
}

fn decode_f32_load_const_base(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_const_base(code, cursor)
}

fn decode_f64_load(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i64_load(code, cursor)
}

fn decode_f64_load_const_base(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F64LoadConstBase {
        memarg: operand_memarg(code, cursor, 1)?,
    })
}

fn decode_i64_load_const_base(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_f64_load_const_base(code, cursor)
}

fn decode_f64_load_local_base(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F64LoadLocalBase {
        local: operand_local(code, cursor, 1)?,
        delta: operand_i32(code, cursor, 2)? as u32,
        memarg: operand_memarg(code, cursor, 3)?,
    })
}

fn decode_i64_load_local_base(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i64_load_local_base_width(code, cursor, 8, false)
}

fn decode_i64_load8_s_local_base(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i64_load_local_base_width(code, cursor, 1, true)
}

fn decode_i64_load8_u_local_base(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i64_load_local_base_width(code, cursor, 1, false)
}

fn decode_i64_load16_s_local_base(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i64_load_local_base_width(code, cursor, 2, true)
}

fn decode_i64_load16_u_local_base(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i64_load_local_base_width(code, cursor, 2, false)
}

fn decode_i64_load32_s_local_base(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i64_load_local_base_width(code, cursor, 4, true)
}

fn decode_i64_load32_u_local_base(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i64_load_local_base_width(code, cursor, 4, false)
}

fn decode_i64_load_local_base_width(
    code: &[Instr],
    cursor: usize,
    width: u32,
    signed: bool,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64LoadLocalBase {
        local: operand_local(code, cursor, 1)?,
        delta: operand_i32(code, cursor, 2)? as u32,
        memarg: operand_memarg(code, cursor, 3)?,
        width,
        signed,
    })
}

fn decode_i32_load_const_base(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32LoadConstBase {
        memarg: operand_memarg(code, cursor, 1)?,
    })
}

fn decode_i32_load_const_base_local_get4_i32_add_set4(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32LoadConstBaseLocalGet4I32AddSet4 {
        memarg: operand_memarg(code, cursor, 1)?,
        rhs: operand_local(code, cursor, 2)?,
        dst: operand_local(code, cursor, 3)?,
    })
}

fn decode_i32_load_store_local_base_local_get4(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    let kind = operand_u32(code, cursor, 1)?;
    Ok(BaselineOp::I32LoadStoreLocalBaseLocalGet4 {
        load_kind: kind & 0xff,
        store_kind: (kind >> 8) & 0xff,
        load_memarg: operand_memarg(code, cursor, 2)?,
        store_addr_local: operand_local(code, cursor, 3)?,
        store_delta: operand_i32(code, cursor, 4)? as u32,
        value_local: operand_local(code, cursor, 5)?,
        store_memarg: operand_memarg(code, cursor, 6)?,
        skip_slots: operand_u32(code, cursor, 7)? as usize,
    })
}

fn decode_i32_load_tee4_br_if(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_tee4_br_if_width(code, cursor, 4, false, false)
}

fn decode_i32_load_tee4_i32_eqz_br_if(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_tee4_br_if_width(code, cursor, 4, false, true)
}

fn decode_i32_load8_u_tee4_br_if(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_tee4_br_if_width(code, cursor, 1, false, false)
}

fn decode_i32_load8_u_tee4_i32_eqz_br_if(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_tee4_br_if_width(code, cursor, 1, false, true)
}

fn decode_i32_load8_s_tee4_br_if(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_tee4_br_if_width(code, cursor, 1, true, false)
}

fn decode_i32_load8_s_tee4_i32_eqz_br_if(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_tee4_br_if_width(code, cursor, 1, true, true)
}

fn decode_i32_load16_u_tee4_br_if(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_tee4_br_if_width(code, cursor, 2, false, false)
}

fn decode_i32_load16_u_tee4_i32_eqz_br_if(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_tee4_br_if_width(code, cursor, 2, false, true)
}

fn decode_i32_load16_s_tee4_br_if(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_tee4_br_if_width(code, cursor, 2, true, false)
}

fn decode_i32_load16_s_tee4_i32_eqz_br_if(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_tee4_br_if_width(code, cursor, 2, true, true)
}

fn decode_i32_load_tee4_br_if_width(
    code: &[Instr],
    cursor: usize,
    width: u32,
    signed: bool,
    eqz: bool,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32LoadTee4BrIf {
        memarg: operand_memarg(code, cursor, 1)?,
        width,
        signed,
        dst: operand_local(code, cursor, 2)?,
        eqz,
        target: skip_end_ops(code, operand_jump_addr(code, cursor, 3)?),
    })
}

fn decode_i32_load_local_base(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_width(code, cursor, 4, false)
}

fn decode_i32_load8_u_local_base(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_width(code, cursor, 1, false)
}

fn decode_i32_load8_s_local_base(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_width(code, cursor, 1, true)
}

fn decode_i32_load16_u_local_base(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_width(code, cursor, 2, false)
}

fn decode_i32_load16_s_local_base(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_width(code, cursor, 2, true)
}

fn decode_i32_load_local_base_tee4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_write4_width(code, cursor, 4, false, true)
}

fn decode_i32_load8_u_local_base_tee4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_write4_width(code, cursor, 1, false, true)
}

fn decode_i32_load8_s_local_base_tee4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_write4_width(code, cursor, 1, true, true)
}

fn decode_i32_load16_u_local_base_tee4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_write4_width(code, cursor, 2, false, true)
}

fn decode_i32_load16_s_local_base_tee4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_write4_width(code, cursor, 2, true, true)
}

fn decode_i32_load_local_base_width(
    code: &[Instr],
    cursor: usize,
    width: u32,
    signed: bool,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32LoadLocalBase {
        local: operand_local(code, cursor, 1)?,
        delta: operand_i32(code, cursor, 2)? as u32,
        memarg: operand_memarg(code, cursor, 3)?,
        width,
        signed,
    })
}

fn decode_i32_load_local_base_set4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_write4_width(code, cursor, 4, false, false)
}

fn decode_i32_load8_u_local_base_set4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_write4_width(code, cursor, 1, false, false)
}

fn decode_i32_load8_u_local_base_set4_local_get4(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32LoadLocalBaseSet4LocalGet4 {
        local: operand_local(code, cursor, 1)?,
        delta: operand_i32(code, cursor, 2)? as u32,
        memarg: operand_memarg(code, cursor, 3)?,
        width: 1,
        signed: false,
        dst: operand_local(code, cursor, 4)?,
        preserved: operand_local(code, cursor, 5)?,
    })
}

fn decode_i32_load8_s_local_base_set4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_write4_width(code, cursor, 1, true, false)
}

fn decode_i32_load16_u_local_base_set4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_write4_width(code, cursor, 2, false, false)
}

fn decode_i32_load16_s_local_base_set4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_write4_width(code, cursor, 2, true, false)
}

fn decode_i32_load_local_base_write4_width(
    code: &[Instr],
    cursor: usize,
    width: u32,
    signed: bool,
    keep_result: bool,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32LoadLocalBaseSet4 {
        local: operand_local(code, cursor, 1)?,
        delta: operand_i32(code, cursor, 2)? as u32,
        memarg: operand_memarg(code, cursor, 3)?,
        width,
        signed,
        dst: operand_local(code, cursor, 4)?,
        keep_result,
    })
}

fn decode_i32_load_local_base_local_get4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_local_get4_width(code, cursor, 4, false)
}

fn decode_i32_load8_u_local_base_local_get4(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_local_get4_width(code, cursor, 1, false)
}

fn decode_i32_load8_s_local_base_local_get4(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_local_get4_width(code, cursor, 1, true)
}

fn decode_i32_load16_u_local_base_local_get4(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_local_get4_width(code, cursor, 2, false)
}

fn decode_i32_load16_s_local_base_local_get4(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_local_get4_width(code, cursor, 2, true)
}

fn decode_i32_load_local_base_local_get4_width(
    code: &[Instr],
    cursor: usize,
    width: u32,
    signed: bool,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32LoadLocalBaseLocalGet4 {
        local: operand_local(code, cursor, 1)?,
        delta: operand_i32(code, cursor, 2)? as u32,
        memarg: operand_memarg(code, cursor, 3)?,
        width,
        signed,
        dst: None,
        preserved: operand_local(code, cursor, 4)?,
    })
}

fn decode_i32_load_local_base_tee4_local_get4(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_tee4_local_get4_width(code, cursor, 4, false)
}

fn decode_i32_load8_u_local_base_tee4_local_get4(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_tee4_local_get4_width(code, cursor, 1, false)
}

fn decode_i32_load8_s_local_base_tee4_local_get4(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_tee4_local_get4_width(code, cursor, 1, true)
}

fn decode_i32_load16_u_local_base_tee4_local_get4(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_tee4_local_get4_width(code, cursor, 2, false)
}

fn decode_i32_load16_s_local_base_tee4_local_get4(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_tee4_local_get4_width(code, cursor, 2, true)
}

fn decode_i32_load_local_base_tee4_local_get4_width(
    code: &[Instr],
    cursor: usize,
    width: u32,
    signed: bool,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32LoadLocalBaseLocalGet4 {
        local: operand_local(code, cursor, 1)?,
        delta: operand_i32(code, cursor, 2)? as u32,
        memarg: operand_memarg(code, cursor, 3)?,
        width,
        signed,
        dst: Some(operand_local(code, cursor, 4)?),
        preserved: operand_local(code, cursor, 5)?,
    })
}

fn decode_i32_load_local_base_set4_i32_load8_u_local_base(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_set4_i32_load_local_base_width(code, cursor, 1, false, false)
}

fn decode_i32_load_local_base_set4_i32_load_local_base(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_set4_i32_load_local_base_width(code, cursor, 4, false, false)
}

fn decode_i32_load_local_base_set4_i32_load8_s_local_base(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_set4_i32_load_local_base_width(code, cursor, 1, true, false)
}

fn decode_i32_load_local_base_set4_i32_load16_u_local_base(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_set4_i32_load_local_base_width(code, cursor, 2, false, false)
}

fn decode_i32_load_local_base_set4_i32_load16_s_local_base(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_set4_i32_load_local_base_width(code, cursor, 2, true, false)
}

fn decode_i32_load_local_base_set4_i32_load8_u_local_base_local_get4(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_set4_i32_load_local_base_width(code, cursor, 1, false, true)
}

fn decode_i32_load_local_base_set4_i32_load8_u_local_base_local_eq_br_if(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32LoadLocalBaseSet4I32LoadLocalBaseEqBrIf {
        first_base_local: operand_local(code, cursor, 1)?,
        first_delta: operand_i32(code, cursor, 2)? as u32,
        first_memarg: operand_memarg(code, cursor, 3)?,
        dst: operand_local(code, cursor, 4)?,
        second_delta: operand_i32(code, cursor, 5)? as u32,
        second_memarg: operand_memarg(code, cursor, 6)?,
        second_width: 1,
        second_signed: false,
        rhs: operand_local(code, cursor, 7)?,
        target: skip_end_ops(code, operand_jump_addr(code, cursor, 8)?),
    })
}

fn decode_i32_load_local_base_set4_i32_load8_s_local_base_local_get4(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_set4_i32_load_local_base_width(code, cursor, 1, true, true)
}

fn decode_i32_load_local_base_set4_i32_load16_u_local_base_local_get4(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_set4_i32_load_local_base_width(code, cursor, 2, false, true)
}

fn decode_i32_load_local_base_set4_i32_load16_s_local_base_local_get4(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_set4_i32_load_local_base_width(code, cursor, 2, true, true)
}

fn decode_i32_load_local_base_set4_i32_load_local_base_width(
    code: &[Instr],
    cursor: usize,
    second_width: u32,
    second_signed: bool,
    has_preserved: bool,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32LoadLocalBaseSet4I32LoadLocalBase {
        first_base_local: operand_local(code, cursor, 1)?,
        first_delta: operand_i32(code, cursor, 2)? as u32,
        first_memarg: operand_memarg(code, cursor, 3)?,
        dst: operand_local(code, cursor, 4)?,
        second_delta: operand_i32(code, cursor, 5)? as u32,
        second_memarg: operand_memarg(code, cursor, 6)?,
        second_width,
        second_signed,
        preserved: if has_preserved {
            Some(operand_local(code, cursor, 7)?)
        } else {
            None
        },
    })
}

fn decode_i32_load16_u_search_loop(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_search_loop(code, cursor, 2, 0xffff, SearchCompare::Eq, true)
}

fn decode_i32_load16_u_search_loop_fallthrough(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_search_loop(code, cursor, 2, 0xffff, SearchCompare::Eq, false)
}

fn decode_i32_load8_u_masked_search_loop(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    let compare = decode_search_compare(operand_u32(code, cursor, 8)?)?;
    decode_i32_search_loop(code, cursor, 1, 0xff, compare, true)
}

fn decode_i32_load8_u_masked_search_loop_fallthrough(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    let compare = decode_search_compare(operand_u32(code, cursor, 8)?)?;
    decode_i32_search_loop(code, cursor, 1, 0xff, compare, false)
}

fn decode_i32_search_loop(
    code: &[Instr],
    cursor: usize,
    field_width: u32,
    rhs_mask: u32,
    compare: SearchCompare,
    has_miss_target: bool,
) -> Result<BaselineOp, ()> {
    let compare_operand_slots = usize::from(field_width == 1);
    let next_delta_operand = 8 + compare_operand_slots;
    let next_memarg_operand = 9 + compare_operand_slots;
    let match_operand = 10 + compare_operand_slots;
    let miss_operand = 11 + compare_operand_slots;
    Ok(BaselineOp::I32LoadLocalBaseSet4SearchLoop {
        node_local: operand_local(code, cursor, 1)?,
        data_delta: operand_i32(code, cursor, 2)? as u32,
        data_memarg: operand_memarg(code, cursor, 3)?,
        data_local: operand_local(code, cursor, 4)?,
        field_delta: operand_i32(code, cursor, 5)? as u32,
        field_memarg: operand_memarg(code, cursor, 6)?,
        field_width,
        rhs_local: operand_local(code, cursor, 7)?,
        rhs_mask,
        compare,
        next_delta: operand_i32(code, cursor, next_delta_operand)? as u32,
        next_memarg: operand_memarg(code, cursor, next_memarg_operand)?,
        match_target: operand_jump_addr(code, cursor, match_operand)?,
        miss_target: if has_miss_target {
            Some(operand_jump_addr(code, cursor, miss_operand)?)
        } else {
            None
        },
    })
}

fn decode_search_compare(kind: u32) -> Result<SearchCompare, ()> {
    match kind {
        0 => Ok(SearchCompare::Eq),
        1 => Ok(SearchCompare::Ne),
        _ => Err(()),
    }
}

fn decode_i32_load_store_local_base_reverse_loop(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32LoadStoreLocalBaseReverseLoop {
        prev_local: operand_local(code, cursor, 1)?,
        saved_local: operand_local(code, cursor, 2)?,
        cursor_local: operand_local(code, cursor, 3)?,
        load_memarg: operand_memarg(code, cursor, 4)?,
        store_memarg: operand_memarg(code, cursor, 5)?,
    })
}

fn decode_i32_load_store_local_base_relink_loop(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32LoadStoreLocalBaseRelinkLoop {
        cursor_local: operand_local(code, cursor, 1)?,
        current_local: operand_local(code, cursor, 2)?,
        prev_local: operand_local(code, cursor, 3)?,
        load_memarg: operand_memarg(code, cursor, 4)?,
        store_memarg: operand_memarg(code, cursor, 5)?,
    })
}

fn decode_i32_load16_u_update_store16_local_base_loop(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Load16UpdateStore16LocalBaseLoop {
        subtract: operand_u32(code, cursor, 1)? & 1 != 0,
        ptr_local: operand_local(code, cursor, 2)?,
        scalar_local: operand_local(code, cursor, 3)?,
        counter_local: operand_local(code, cursor, 4)?,
        load_delta: operand_i32(code, cursor, 5)? as u32,
        store_delta: operand_i32(code, cursor, 6)? as u32,
        load_memarg: operand_memarg(code, cursor, 7)?,
        store_memarg: operand_memarg(code, cursor, 8)?,
    })
}

fn decode_i32_load16_u_local_base_local_get4_i32_load16_u(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_local_get4_i32_load_width(code, cursor, 2, false, 2, false)
}

fn decode_i32_load16_s_local_base_local_get4_i32_load16_s(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_local_get4_i32_load_width(code, cursor, 2, true, 2, true)
}

fn decode_i32_load_local_base_local_get4_i32_load_width(
    code: &[Instr],
    cursor: usize,
    first_width: u32,
    first_signed: bool,
    second_width: u32,
    second_signed: bool,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32LoadLocalBaseLocalGet4I32Load {
        first_base_local: operand_local(code, cursor, 1)?,
        first_delta: operand_i32(code, cursor, 2)? as u32,
        first_memarg: operand_memarg(code, cursor, 3)?,
        first_width,
        first_signed,
        second_addr_local: operand_local(code, cursor, 4)?,
        second_memarg: operand_memarg(code, cursor, 5)?,
        second_width,
        second_signed,
    })
}

fn decode_i32_load_local_base_local_get4_i32_load_tee4_cmp_br_if(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    let kind = operand_u32(code, cursor, 1)?;
    let (first_width, first_signed) = decode_i32_scalar_load_kind(kind & 0xff)?;
    let (second_width, second_signed) = decode_i32_scalar_load_kind((kind >> 8) & 0xff)?;
    Ok(BaselineOp::I32LoadLocalBaseLocalGet4I32LoadCmpBrIf {
        first_base_local: operand_local(code, cursor, 2)?,
        first_delta: operand_i32(code, cursor, 3)? as u32,
        first_memarg: operand_memarg(code, cursor, 4)?,
        first_width,
        first_signed,
        first_dst: operand_local(code, cursor, 5)?,
        second_addr_local: operand_local(code, cursor, 6)?,
        second_memarg: operand_memarg(code, cursor, 7)?,
        second_width,
        second_signed,
        second_dst: operand_local(code, cursor, 8)?,
        compare: decode_i32_compare_kind((kind >> 16) & 0xff)?,
        target: skip_end_ops(code, operand_jump_addr(code, cursor, 9)?),
    })
}

fn decode_i32_scalar_load_kind(kind: u32) -> Result<(u32, bool), ()> {
    match kind {
        0 => Ok((4, false)),
        1 => Ok((1, true)),
        2 => Ok((1, false)),
        3 => Ok((2, true)),
        4 => Ok((2, false)),
        _ => Err(()),
    }
}

fn decode_i32_compare_kind(kind: u32) -> Result<I32CompareOp, ()> {
    match kind {
        0 => Ok(I32CompareOp::Eq),
        1 => Ok(I32CompareOp::Ne),
        2 => Ok(I32CompareOp::LtS),
        3 => Ok(I32CompareOp::LtU),
        4 => Ok(I32CompareOp::GtS),
        5 => Ok(I32CompareOp::GtU),
        6 => Ok(I32CompareOp::LeS),
        7 => Ok(I32CompareOp::LeU),
        8 => Ok(I32CompareOp::GeS),
        9 => Ok(I32CompareOp::GeU),
        _ => Err(()),
    }
}

fn decode_i32_load16_u_local_scaled_index(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_local_scaled_index_width(code, cursor, 2, false)
}

fn decode_i32_load16_s_local_scaled_index(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_local_scaled_index_width(code, cursor, 2, true)
}

fn decode_i32_load8_u_local_scaled_index(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_local_scaled_index_width(code, cursor, 1, false)
}

fn decode_i32_load8_s_local_scaled_index(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_local_scaled_index_width(code, cursor, 1, true)
}

fn decode_i32_load_local_scaled_index_width(
    code: &[Instr],
    cursor: usize,
    width: u32,
    signed: bool,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32LoadLocalScaledIndex {
        base_local: operand_local(code, cursor, 1)?,
        index_local: operand_local(code, cursor, 2)?,
        scale_log2: operand_u32(code, cursor, 3)?,
        delta: operand_i32(code, cursor, 4)? as u32,
        memarg: operand_memarg(code, cursor, 5)?,
        width,
        signed,
    })
}

fn decode_i32_store(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_store_width(code, cursor, 4)
}

fn decode_i32_store8(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_store_width(code, cursor, 1)
}

fn decode_i32_store16(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_store_width(code, cursor, 2)
}

fn decode_i32_store_width(code: &[Instr], cursor: usize, width: u32) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Store {
        memarg: operand_memarg(code, cursor, 1)?,
        width,
    })
}

fn decode_f32_store(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_store_width(code, cursor, 4)
}

fn decode_i64_store(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i64_store_width(code, cursor, 8)
}

fn decode_f64_store(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i64_store_width(code, cursor, 8)
}

fn decode_i64_store8(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i64_store_width(code, cursor, 1)
}

fn decode_i64_store16(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i64_store_width(code, cursor, 2)
}

fn decode_i64_store32(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i64_store_width(code, cursor, 4)
}

fn decode_i64_store_width(code: &[Instr], cursor: usize, width: u32) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64Store {
        memarg: operand_memarg(code, cursor, 1)?,
        width,
    })
}

fn decode_i64_store_local_base(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i64_store_local_base_width(code, cursor, 8)
}

fn decode_i64_store8_local_base(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i64_store_local_base_width(code, cursor, 1)
}

fn decode_i64_store16_local_base(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i64_store_local_base_width(code, cursor, 2)
}

fn decode_i64_store32_local_base(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i64_store_local_base_width(code, cursor, 4)
}

fn decode_i64_store_local_base_width(
    code: &[Instr],
    cursor: usize,
    width: u32,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I64StoreLocalBase {
        base_local: operand_local(code, cursor, 1)?,
        delta: operand_i32(code, cursor, 2)? as u32,
        memarg: operand_memarg(code, cursor, 3)?,
        width,
    })
}

fn decode_f64_store_local_base(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::F64StoreLocalBase {
        base_local: operand_local(code, cursor, 1)?,
        delta: operand_i32(code, cursor, 2)? as u32,
        memarg: operand_memarg(code, cursor, 3)?,
    })
}

fn decode_store_const_base_local4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::StoreConstBaseLocal4 {
        memarg: operand_memarg(code, cursor, 1)?,
        local: operand_local(code, cursor, 2)?,
    })
}

fn decode_store_const_base_local8(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::StoreConstBaseLocal8 {
        memarg: operand_memarg(code, cursor, 1)?,
        local: operand_local(code, cursor, 2)?,
    })
}

fn decode_i32_store_local_base(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_store_local_base_width(code, cursor, 4)
}

fn decode_i32_store8_local_base(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_store_local_base_width(code, cursor, 1)
}

fn decode_i32_store16_local_base(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_store_local_base_width(code, cursor, 2)
}

fn decode_i32_store_local_base_width(
    code: &[Instr],
    cursor: usize,
    width: u32,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32StoreLocalBase {
        base_local: operand_local(code, cursor, 1)?,
        delta: operand_i32(code, cursor, 2)? as u32,
        memarg: operand_memarg(code, cursor, 3)?,
        width,
    })
}

fn decode_i32_store_local_base_local_get4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_store_local_base_local_get4_width(code, cursor, 4)
}

fn decode_i32_store8_local_base_local_get4(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_store_local_base_local_get4_width(code, cursor, 1)
}

fn decode_i32_store16_local_base_local_get4(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_store_local_base_local_get4_width(code, cursor, 2)
}

fn decode_i32_store_local_base_local_get4_width(
    code: &[Instr],
    cursor: usize,
    width: u32,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32StoreLocalBaseLocalGet4 {
        addr_local: operand_local(code, cursor, 1)?,
        delta: operand_i32(code, cursor, 2)? as u32,
        value_local: operand_local(code, cursor, 3)?,
        memarg: operand_memarg(code, cursor, 4)?,
        width,
    })
}

fn decode_i32_store_local_scaled_index(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_store_local_scaled_index_width(code, cursor, 4)
}

fn decode_i32_store8_local_scaled_index(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_store_local_scaled_index_width(code, cursor, 1)
}

fn decode_i32_store16_local_scaled_index(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_store_local_scaled_index_width(code, cursor, 2)
}

fn decode_i32_store_local_scaled_index_width(
    code: &[Instr],
    cursor: usize,
    width: u32,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32StoreLocalScaledIndex {
        base_local: operand_local(code, cursor, 1)?,
        index_local: operand_local(code, cursor, 2)?,
        scale_log2: operand_u32(code, cursor, 3)?,
        delta: operand_i32(code, cursor, 4)? as u32,
        memarg: operand_memarg(code, cursor, 5)?,
        width,
    })
}

fn decode_i32_inc_local_base(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32IncLocalBase {
        base_local: operand_local(code, cursor, 1)?,
        store_delta: operand_i32(code, cursor, 2)? as u32,
        load_delta: operand_i32(code, cursor, 3)? as u32,
        load_memarg: operand_memarg(code, cursor, 4)?,
        store_memarg: operand_memarg(code, cursor, 5)?,
    })
}

fn decode_i32_load_local_base_tee4_i32_load8_u_tee4_br_if(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32LoadLocalBaseTeeLoad8UTeeBrIf {
        first_base_local: operand_local(code, cursor, 1)?,
        first_delta: operand_i32(code, cursor, 2)? as u32,
        first_memarg: operand_memarg(code, cursor, 3)?,
        first_dst: operand_local(code, cursor, 4)?,
        byte_memarg: operand_memarg(code, cursor, 5)?,
        byte_dst: operand_local(code, cursor, 6)?,
        target: skip_end_ops(code, operand_jump_addr(code, cursor, 7)?),
    })
}

fn decode_memory_fill(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::MemoryFill)
}

fn decode_memory_copy(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::MemoryCopy)
}

fn decode_mem_size(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::MemorySize { shared: false })
}

fn decode_mem_size_shared(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::MemorySize { shared: true })
}

fn decode_mem_grow(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::MemoryGrow { shared: false })
}

fn decode_mem_grow_shared(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::MemoryGrow { shared: true })
}

fn decode_scalar_copy_local_base_run(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    let kind = operand_u32(code, cursor, 1)?;
    let width = kind & 0xff;
    let count = (kind >> 8) & 0xff;
    if !matches!(width, 1 | 2 | 4) || count == 0 || count > 16 {
        return Err(());
    }
    let mut lanes = Vec::with_capacity(count as usize);
    let mut operand_offset = 4usize;
    for _ in 0..count {
        lanes.push(ScalarCopyLane {
            dst_delta: operand_i32(code, cursor, operand_offset)? as u32,
            src_delta: operand_i32(code, cursor, operand_offset + 1)? as u32,
            load_memarg: operand_memarg(code, cursor, operand_offset + 2)?,
            store_memarg: operand_memarg(code, cursor, operand_offset + 3)?,
        });
        operand_offset += 4;
    }
    Ok(BaselineOp::ScalarCopyLocalBaseRun {
        width,
        dst_base_local: operand_local(code, cursor, 2)?,
        src_base_local: operand_local(code, cursor, 3)?,
        lanes,
    })
}

fn decode_i32_load_local_base_tee4_br_if(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_tee4_br_if_width(code, cursor, 4, false, false)
}

fn decode_i32_load_local_base_tee4_i32_eqz_br_if(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_tee4_br_if_width(code, cursor, 4, false, true)
}

fn decode_i32_load8_u_local_base_tee4_br_if(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_tee4_br_if_width(code, cursor, 1, false, false)
}

fn decode_i32_load8_u_local_base_tee4_i32_eqz_br_if(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_tee4_br_if_width(code, cursor, 1, false, true)
}

fn decode_i32_load8_s_local_base_tee4_br_if(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_tee4_br_if_width(code, cursor, 1, true, false)
}

fn decode_i32_load8_s_local_base_tee4_i32_eqz_br_if(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_tee4_br_if_width(code, cursor, 1, true, true)
}

fn decode_i32_load16_u_local_base_tee4_br_if(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_tee4_br_if_width(code, cursor, 2, false, false)
}

fn decode_i32_load16_u_local_base_tee4_i32_eqz_br_if(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_tee4_br_if_width(code, cursor, 2, false, true)
}

fn decode_i32_load16_s_local_base_tee4_br_if(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_tee4_br_if_width(code, cursor, 2, true, false)
}

fn decode_i32_load16_s_local_base_tee4_i32_eqz_br_if(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    decode_i32_load_local_base_tee4_br_if_width(code, cursor, 2, true, true)
}

fn decode_i32_load_local_base_tee4_br_if_width(
    code: &[Instr],
    cursor: usize,
    width: u32,
    signed: bool,
    eqz: bool,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32LoadLocalBaseTee4BrIf {
        base_local: operand_local(code, cursor, 1)?,
        delta: operand_i32(code, cursor, 2)? as u32,
        memarg: operand_memarg(code, cursor, 3)?,
        width,
        signed,
        dst: operand_local(code, cursor, 4)?,
        eqz,
        target: skip_end_ops(code, operand_jump_addr(code, cursor, 5)?),
    })
}

fn decode_i32_guarded_load8_update_br_if(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    let guard_kind = operand_u32(code, cursor, 4)?;
    decode_local_cmp32_kind(guard_kind).ok_or(())?;
    Ok(BaselineOp::I32GuardedLoad8UpdateBrIf {
        next_src: operand_local(code, cursor, 1)?,
        next_delta: operand_i32(code, cursor, 2)? as u32,
        next_dst: operand_local(code, cursor, 3)?,
        guard_kind,
        guard_lhs: operand_local(code, cursor, 5)?,
        guard_rhs: operand_u32(code, cursor, 6)?,
        false_target: skip_end_ops(code, operand_jump_addr(code, cursor, 7)?),
        ptr_local: operand_local(code, cursor, 8)?,
        load_delta: operand_i32(code, cursor, 9)? as u32,
        memarg: operand_memarg(code, cursor, 10)?,
        byte_dst: operand_local(code, cursor, 11)?,
        update_src: operand_local(code, cursor, 12)?,
        ptr_dst: operand_local(code, cursor, 13)?,
        branch_local: operand_local(code, cursor, 14)?,
        true_target: skip_end_ops(code, operand_jump_addr(code, cursor, 15)?),
    })
}

fn decode_i32_load8_update_br_if(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Load8UpdateBrIf {
        ptr_local: operand_local(code, cursor, 1)?,
        load_delta: operand_i32(code, cursor, 2)? as u32,
        memarg: operand_memarg(code, cursor, 3)?,
        byte_dst: operand_local(code, cursor, 4)?,
        next_src: operand_local(code, cursor, 5)?,
        ptr_dst: operand_local(code, cursor, 6)?,
        branch_local: operand_local(code, cursor, 7)?,
        target: skip_end_ops(code, operand_jump_addr(code, cursor, 8)?),
    })
}

fn decode_local_add_set_load8_eqz_br_if(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalAddSetLoad8EqzBrIf {
        add_src: operand_local(code, cursor, 1)?,
        imm: operand_i32(code, cursor, 2)? as u32,
        add_dst: operand_local(code, cursor, 3)?,
        load_base: operand_local(code, cursor, 4)?,
        load_delta: operand_i32(code, cursor, 5)? as u32,
        memarg: operand_memarg(code, cursor, 6)?,
        tee_dst: operand_local(code, cursor, 7)?,
        target: skip_end_ops(code, operand_jump_addr(code, cursor, 8)?),
    })
}

fn decode_branch(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::Branch {
        target: operand_jump_addr(code, cursor, 1)?,
    })
}

fn decode_br_if(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::BrIf {
        target: operand_jump_addr(code, cursor, 1)?,
    })
}

fn decode_local_get4_br_if(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalGet4BrIf {
        local: operand_local(code, cursor, 1)?,
        target: operand_jump_addr(code, cursor, 2)?,
    })
}

fn decode_local_get4_i32_const_add_br_if(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalGet4I32ConstAddBrIf {
        local: operand_local(code, cursor, 1)?,
        imm: operand_i32(code, cursor, 2)? as u32,
        target: operand_jump_addr(code, cursor, 3)?,
    })
}

fn decode_local_get4_local_get4_i32_add_br_if(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalGet4LocalGet4I32AddBrIf {
        lhs: operand_local(code, cursor, 1)?,
        rhs: operand_local(code, cursor, 2)?,
        target: operand_jump_addr(code, cursor, 3)?,
    })
}

fn decode_local_get4_i32_eqz_br_if(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalGet4I32EqzBrIf {
        local: operand_local(code, cursor, 1)?,
        target: operand_jump_addr(code, cursor, 2)?,
    })
}

fn decode_local_get4_i32_const_compare_br_if(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalGet4I32ConstCompareBrIf {
        local: operand_local(code, cursor, 1)?,
        kind: operand_u32(code, cursor, 2)?,
        rhs: operand_i32(code, cursor, 3)? as u32,
        target: operand_jump_addr(code, cursor, 4)?,
    })
}

fn decode_local_get4_local_get4_compare_br_if(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalGet4LocalGet4CompareBrIf {
        lhs: operand_local(code, cursor, 1)?,
        rhs: operand_local(code, cursor, 2)?,
        kind: operand_u32(code, cursor, 3)?,
        target: operand_jump_addr(code, cursor, 4)?,
    })
}

fn decode_local_get4_i32_const_and_br_if(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalGet4I32ConstAndBrIf {
        local: operand_local(code, cursor, 1)?,
        mask: operand_i32(code, cursor, 2)? as u32,
        eqz: false,
        target: operand_jump_addr(code, cursor, 3)?,
    })
}

fn decode_local_get4_i32_const_and_eqz_br_if(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalGet4I32ConstAndBrIf {
        local: operand_local(code, cursor, 1)?,
        mask: operand_i32(code, cursor, 2)? as u32,
        eqz: true,
        target: operand_jump_addr(code, cursor, 3)?,
    })
}

fn decode_local_get4_i32_const_and_i32_const_compare_br_if(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalGet4I32ConstAndI32ConstCompareBrIf {
        local: operand_local(code, cursor, 1)?,
        mask: operand_i32(code, cursor, 2)? as u32,
        kind: operand_u32(code, cursor, 3)?,
        rhs: operand_i32(code, cursor, 4)? as u32,
        target: operand_jump_addr(code, cursor, 5)?,
    })
}

fn decode_local_get4_i32_const_and_tee4_i32_const_eq_br_if(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalGet4I32ConstAndTee4I32ConstEqBrIf {
        local: operand_local(code, cursor, 1)?,
        mask: operand_i32(code, cursor, 2)? as u32,
        dst: operand_local(code, cursor, 3)?,
        rhs: operand_i32(code, cursor, 4)? as u32,
        target: operand_jump_addr(code, cursor, 5)?,
    })
}

fn decode_local_get4_set4_local_get4_i32_const_compare_br_if(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalGet4Set4LocalGet4I32ConstCompareBrIf {
        copy_src: operand_local(code, cursor, 1)?,
        copy_dst: operand_local(code, cursor, 2)?,
        lhs: operand_local(code, cursor, 3)?,
        kind: operand_u32(code, cursor, 4)?,
        rhs: operand_i32(code, cursor, 5)? as u32,
        target: operand_jump_addr(code, cursor, 6)?,
    })
}

fn decode_local_get4_i32_const_add_i32_const_and_i32_const_compare_br_if(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    Ok(
        BaselineOp::LocalGet4I32ConstAddI32ConstAndI32ConstCompareBrIf {
            local: operand_local(code, cursor, 1)?,
            imm: operand_i32(code, cursor, 2)? as u32,
            mask: operand_i32(code, cursor, 3)? as u32,
            kind: operand_u32(code, cursor, 4)?,
            rhs: operand_i32(code, cursor, 5)? as u32,
            target: operand_jump_addr(code, cursor, 6)?,
        },
    )
}

fn decode_local_get4_i32_const_add_tee4_br_if(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalGet4I32ConstAddTee4BrIf {
        src: operand_local(code, cursor, 1)?,
        imm: operand_i32(code, cursor, 2)? as u32,
        dst: operand_local(code, cursor, 3)?,
        target: operand_jump_addr(code, cursor, 4)?,
    })
}

fn decode_local_get4_br_table(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalGet4BrTable {
        local: operand_local(code, cursor, 1)?,
        addend: 0,
        targets: decode_br_table_targets(code, cursor, 2, 3)?,
    })
}

fn decode_local_get4_i32_const_add_br_table(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::LocalGet4BrTable {
        local: operand_local(code, cursor, 1)?,
        addend: operand_i32(code, cursor, 2)? as u32,
        targets: decode_br_table_targets(code, cursor, 3, 4)?,
    })
}

fn decode_br_table(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::BrTable {
        targets: decode_br_table_targets(code, cursor, 1, 2)?,
    })
}

fn decode_if(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::If {
        else_target: operand_jump_addr(code, cursor, 1)?,
    })
}

fn decode_else(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::Else {
        target: operand_jump_addr(code, cursor, 1)?.saturating_add(1),
    })
}

fn decode_br_table_targets(
    code: &[Instr],
    cursor: usize,
    table_size_offset: usize,
    first_target_offset: usize,
) -> Result<Vec<usize>, ()> {
    let table_size = operand_u32(code, cursor, table_size_offset)? as usize;
    let mut targets = Vec::with_capacity(table_size.saturating_add(1));
    for offset in first_target_offset..=first_target_offset.saturating_add(table_size) {
        targets.push(skip_end_ops(code, operand_jump_addr(code, cursor, offset)?));
    }
    Ok(targets)
}

fn skip_end_ops(code: &[Instr], mut target: usize) -> usize {
    while target < code.len() {
        let op = unsafe { code[target].op };
        if !std::ptr::fn_addr_eq(op, op_end as Op) {
            break;
        }
        target += 1;
    }
    target
}

fn decode_end(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    let next_is_function_return = code.get(cursor + 1).is_some_and(|instr| {
        std::ptr::fn_addr_eq(unsafe { instr.op }, special_function_return as Op)
    });
    let next_resets_stack = if code
        .get(cursor + 1)
        .is_some_and(|instr| std::ptr::fn_addr_eq(unsafe { instr.op }, special_block_return as Op))
    {
        let block_return = unsafe { instr(code, cursor + 2)?.operand.block_return };
        block_return.stack_top == 0 && block_return.return_size == 0
    } else {
        false
    };
    Ok(BaselineOp::End {
        next_is_function_return,
        next_resets_stack,
    })
}

fn decode_block_return(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::BlockReturn {
        block_return: unsafe { instr(code, cursor + 1)?.operand.block_return },
    })
}

fn decode_loop(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::Loop {
        param: operand_loop_param(code, cursor, 1)?,
    })
}

fn decode_function_return(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::FunctionReturn {
        return_size: unsafe { instr(code, cursor + 1)?.operand.drop_size },
    })
}

fn decode_function_vm_end(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::FunctionVmEnd)
}

fn decode_i32_crc16_update16(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_crc16_update16_inner(code, cursor, false)
}

fn decode_i32_crc16_update16_masked(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    decode_i32_crc16_update16_inner(code, cursor, true)
}

fn decode_i32_crc16_update16_inner(
    code: &[Instr],
    cursor: usize,
    masked: bool,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32Crc16Update16 {
        data_local: operand_local(code, cursor, 1)?,
        crc_local: operand_local(code, cursor, 2)?,
        return_target: operand_jump_addr(code, cursor, 3)?,
        masked,
    })
}

fn decode_i32_core_state_benchmark(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32CoreStateBenchmark {
        locals: [
            operand_local(code, cursor, 1)?,
            operand_local(code, cursor, 2)?,
            operand_local(code, cursor, 3)?,
            operand_local(code, cursor, 4)?,
            operand_local(code, cursor, 5)?,
            operand_local(code, cursor, 6)?,
        ],
        return_target: operand_jump_addr(code, cursor, 7)?,
    })
}

fn decode_i32_numeric_token_state_transition(
    code: &[Instr],
    cursor: usize,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32NumericTokenStateTransition {
        instr_ref_local: operand_local(code, cursor, 1)?,
        counts_local: operand_local(code, cursor, 2)?,
        return_target: operand_jump_addr(code, cursor, 3)?,
    })
}

fn decode_i32_list_crc_pair_loop(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32ListCrcPairLoop {
        frame_base_local: operand_local(code, cursor, 1)?,
        res_delta: operand_u32(code, cursor, 2)?,
        iterations_delta: operand_u32(code, cursor, 3)?,
        crc_delta: operand_u32(code, cursor, 4)?,
        target: operand_jump_addr(code, cursor, 5)?,
    })
}

fn decode_i32_list_crc_summary(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32ListCrcSummary {
        res_local: operand_local(code, cursor, 1)?,
        finder_idx_local: operand_local(code, cursor, 2)?,
        return_target: operand_jump_addr(code, cursor, 3)?,
    })
}

fn decode_i32_select_bit_step4(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::I32SelectBitStep4 {
        step: decode_select_bit_step4_at(code, cursor + 1)?,
    })
}

fn decode_i32_select_bit_step4_run(code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    let count = operand_u32(code, cursor, 1)? as usize;
    let mut steps = Vec::with_capacity(count);
    let mut step_cursor = cursor + 2;
    for _ in 0..count {
        steps.push(decode_select_bit_step4_at(code, step_cursor)?);
        step_cursor += 7;
    }
    Ok(BaselineOp::I32SelectBitStep4Run { steps })
}

fn decode_select_bit_step4_at(code: &[Instr], cursor: usize) -> Result<SelectBitStep4, ()> {
    Ok(SelectBitStep4 {
        tmp_local: operand_local(code, cursor - 1, 1)?,
        poly: operand_i32(code, cursor - 1, 2)? as u32,
        source_local: operand_local(code, cursor - 1, 3)?,
        source_shift: operand_u32(code, cursor - 1, 4)?,
        prev_local: operand_local(code, cursor - 1, 5)?,
        flags: operand_u32(code, cursor - 1, 6)?,
        dst_local: operand_local(code, cursor - 1, 7)?,
    })
}

fn decode_call_i32_crc16_update16(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::CallI32Crc16Update16 { masked: false })
}

fn decode_call_i32_crc16_update16_masked(
    _code: &[Instr],
    _cursor: usize,
) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::CallI32Crc16Update16 { masked: true })
}

fn decode_call_i32_list_crc_summary(_code: &[Instr], _cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::CallI32ListCrcSummary)
}

fn decode_call(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::DirectCall {
        operand_index: cursor + 1,
        continuation_index: cursor + 2,
        is_return_call: false,
    })
}

fn decode_return_call(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::DirectCall {
        operand_index: cursor + 1,
        continuation_index: cursor + 2,
        is_return_call: true,
    })
}

fn decode_call_indirect(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::IndirectCall {
        operand_index: cursor + 1,
        continuation_index: cursor + 3,
        is_return_call: false,
    })
}

fn decode_return_call_indirect(_code: &[Instr], cursor: usize) -> Result<BaselineOp, ()> {
    Ok(BaselineOp::IndirectCall {
        operand_index: cursor + 1,
        continuation_index: cursor + 3,
        is_return_call: true,
    })
}

fn operand_u32(code: &[Instr], cursor: usize, offset: usize) -> Result<u32, ()> {
    Ok(unsafe { instr(code, cursor + offset)?.operand.u32 })
}

fn operand_i32(code: &[Instr], cursor: usize, offset: usize) -> Result<i32, ()> {
    Ok(unsafe { instr(code, cursor + offset)?.operand.i32 })
}

fn operand_i64(code: &[Instr], cursor: usize, offset: usize) -> Result<i64, ()> {
    Ok(unsafe { instr(code, cursor + offset)?.operand.i64 })
}

fn operand_f32_bits(code: &[Instr], cursor: usize, offset: usize) -> Result<u32, ()> {
    Ok(unsafe { instr(code, cursor + offset)?.operand.f32.to_bits() })
}

fn operand_f64_bits(code: &[Instr], cursor: usize, offset: usize) -> Result<u64, ()> {
    Ok(unsafe { instr(code, cursor + offset)?.operand.f64.to_bits() })
}

fn operand_local(code: &[Instr], cursor: usize, offset: usize) -> Result<u32, ()> {
    Ok(unsafe { instr(code, cursor + offset)?.operand.local_addr })
}

fn operand_memarg(code: &[Instr], cursor: usize, offset: usize) -> Result<MemArg, ()> {
    Ok(unsafe { instr(code, cursor + offset)?.operand.memarg })
}

fn operand_jump_addr(code: &[Instr], cursor: usize, offset: usize) -> Result<usize, ()> {
    Ok(unsafe { instr(code, cursor + offset)?.operand.jump_addr as usize })
}

fn operand_loop_param(code: &[Instr], cursor: usize, offset: usize) -> Result<LoopParam, ()> {
    Ok(unsafe { instr(code, cursor + offset)?.operand.loop_param })
}

fn instr(code: &[Instr], index: usize) -> Result<&Instr, ()> {
    code.get(index).ok_or(())
}
