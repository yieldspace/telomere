mod cfg;
mod expr;
mod pass;
mod sink;

use crate::common::{FuncIdx, FuncType, Instr, LocalsData};

pub(crate) use cfg::InstructionMeta;

pub(crate) struct OptimizedFunction {
    pub(crate) instrs: Vec<Instr>,
    pub(crate) op_lens: Vec<u16>,
}

pub(crate) fn optimize_function(
    funcidx: FuncIdx,
    functype: &FuncType,
    locals: &mut LocalsData,
    instrs: Vec<Instr>,
    meta: Vec<InstructionMeta>,
) -> OptimizedFunction {
    pass::optimize_function(funcidx, functype, locals, instrs, meta)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{
        cfg::{build_program, InstructionMeta},
        pass::{patch_jump_targets, specialized_memory_family},
        sink::RecordEmit,
    };
    use crate::{
        common::{
            decode_local_binop32_kind, decode_local_binop64_kind, decode_local_cmp32_kind,
            decode_local_cmp64_kind, decode_local_unary32_kind, decode_local_unary64_kind, Func,
            FunctionBody, Instr, LocalBinop32Op, LocalBinop64Op, LocalCmp32Op, LocalCmp64Op,
            LocalFastRhsShape, LocalUnary32Op, LocalUnary64Op, LoopParam, Operand, ValType,
        },
        parser::core::type_checker::StackSnapshot,
        runtime::vm,
        IoReadBinaryReader, WasmParser,
    };

    fn function_at(wat: &str, func_idx: usize) -> Func {
        let bytes = wat::parse_str(wat).expect("wat must parse");
        let mut reader = IoReadBinaryReader::from(bytes.as_slice());
        let mut parser = WasmParser::new(&mut reader);
        let module = parser.parse_module().expect("module must parse");
        let FunctionBody::Wasm(func) = &module.codes.0[func_idx] else {
            panic!("expected wasm function body");
        };
        func.clone()
    }

    fn function_expr_at(wat: &str, func_idx: usize) -> Vec<Instr> {
        function_at(wat, func_idx).expr
    }

    fn function_expr(wat: &str) -> Vec<Instr> {
        function_expr_at(wat, 0)
    }

    fn module_prefix(wast: &str) -> String {
        let (module, _) = wast
            .split_once("\n)\n\n(assert")
            .expect("wast fixture must contain a module followed by asserts");
        format!("{module}\n)")
    }

    fn decoded_ops(expr: &[Instr]) -> Vec<crate::common::Op> {
        let mut ops = Vec::new();
        let mut cursor = 0usize;
        while cursor < expr.len() {
            let op = unsafe { expr[cursor].op };
            ops.push(op);
            cursor += 1 + operand_width(op);
        }
        ops
    }

    fn count_op(expr: &[Instr], op: crate::common::Op) -> usize {
        decoded_ops(expr)
            .into_iter()
            .filter(|candidate| std::ptr::fn_addr_eq(*candidate, op))
            .count()
    }

    fn count_i32_add_family(expr: &[Instr]) -> usize {
        let family = [
            vm::op_i32_add as crate::common::Op,
            vm::op_local_binop32 as crate::common::Op,
            vm::op_local_binop32_set4 as crate::common::Op,
            vm::op_local_binop32_tee4 as crate::common::Op,
            vm::op_local_binop32_br_if as crate::common::Op,
            vm::op_local_get4_i32_const_add as crate::common::Op,
            vm::op_local_get4_i32_const_add_set4 as crate::common::Op,
            vm::op_local_get4_i32_const_add_tee4 as crate::common::Op,
            vm::op_local_get4_local_get4_i32_add as crate::common::Op,
            vm::op_local_get4_local_get4_i32_add_set4 as crate::common::Op,
            vm::op_local_get4_local_get4_i32_add_tee4 as crate::common::Op,
        ];
        decoded_ops(expr)
            .into_iter()
            .filter(|candidate| {
                family
                    .iter()
                    .any(|op| std::ptr::fn_addr_eq(*candidate, *op))
            })
            .count()
    }

    fn count_i32_load_family(expr: &[Instr]) -> usize {
        let family = [
            vm::op_i32_load as crate::common::Op,
            vm::op_i32_load_shared as crate::common::Op,
            vm::op_i32_load_local as crate::common::Op,
            vm::op_i32_load_indexed_local as crate::common::Op,
            vm::op_i32_load_indexed_shared as crate::common::Op,
            vm::op_i32_load_local_base as crate::common::Op,
            vm::op_i32_load_shared_local_base as crate::common::Op,
            vm::op_i32_load_indexed_local_base as crate::common::Op,
            vm::op_i32_load_indexed_shared_local_base as crate::common::Op,
            vm::op_i32_load_local_scaled_index as crate::common::Op,
            vm::op_i32_load_shared_local_scaled_index as crate::common::Op,
            vm::op_i32_load_indexed_local_scaled_index as crate::common::Op,
            vm::op_i32_load_indexed_shared_local_scaled_index as crate::common::Op,
        ];
        decoded_ops(expr)
            .into_iter()
            .filter(|candidate| {
                family
                    .iter()
                    .any(|op| std::ptr::fn_addr_eq(*candidate, *op))
            })
            .count()
    }

    fn count_i32_load8_u_family(expr: &[Instr]) -> usize {
        let family = [
            vm::op_i32_load8_u as crate::common::Op,
            vm::op_i32_load8_u_shared as crate::common::Op,
            vm::op_i32_load8_u_local as crate::common::Op,
            vm::op_i32_load8_u_indexed_local as crate::common::Op,
            vm::op_i32_load8_u_indexed_shared as crate::common::Op,
            vm::op_i32_load8_u_local_base as crate::common::Op,
            vm::op_i32_load8_u_shared_local_base as crate::common::Op,
            vm::op_i32_load8_u_indexed_local_base as crate::common::Op,
            vm::op_i32_load8_u_indexed_shared_local_base as crate::common::Op,
            vm::op_i32_load8_u_local_scaled_index as crate::common::Op,
            vm::op_i32_load8_u_shared_local_scaled_index as crate::common::Op,
            vm::op_i32_load8_u_indexed_local_scaled_index as crate::common::Op,
            vm::op_i32_load8_u_indexed_shared_local_scaled_index as crate::common::Op,
        ];
        decoded_ops(expr)
            .into_iter()
            .filter(|candidate| {
                family
                    .iter()
                    .any(|op| std::ptr::fn_addr_eq(*candidate, *op))
            })
            .count()
    }

    fn debug_op_name(op: crate::common::Op) -> &'static str {
        let names = [
            ("op_global_get4", vm::op_global_get4 as crate::common::Op),
            ("op_global_get16", vm::op_global_get16 as crate::common::Op),
            ("op_global_set16", vm::op_global_set16 as crate::common::Op),
            ("op_drop", vm::op_drop as crate::common::Op),
            ("op_local_get4", vm::op_local_get4 as crate::common::Op),
            ("op_local_get8", vm::op_local_get8 as crate::common::Op),
            ("op_local_get16", vm::op_local_get16 as crate::common::Op),
            ("op_local_set4", vm::op_local_set4 as crate::common::Op),
            ("op_local_tee4", vm::op_local_tee4 as crate::common::Op),
            (
                "f32x4_replace_lane",
                vm::simd::f32x4_replace_lane as crate::common::Op,
            ),
            (
                "f64x2_replace_lane",
                vm::simd::f64x2_replace_lane as crate::common::Op,
            ),
            ("op_i32_const", vm::op_i32_const as crate::common::Op),
            ("op_i32_add", vm::op_i32_add as crate::common::Op),
            ("op_i32_sub", vm::op_i32_sub as crate::common::Op),
            ("op_i32_div_s", vm::op_i32_div_s as crate::common::Op),
            (
                "op_local_binop32",
                vm::op_local_binop32 as crate::common::Op,
            ),
            (
                "op_local_binop32_set4",
                vm::op_local_binop32_set4 as crate::common::Op,
            ),
            (
                "op_local_binop32_tee4",
                vm::op_local_binop32_tee4 as crate::common::Op,
            ),
            (
                "op_local_binop32_br_if",
                vm::op_local_binop32_br_if as crate::common::Op,
            ),
            (
                "op_local_binop64",
                vm::op_local_binop64 as crate::common::Op,
            ),
            (
                "op_local_binop64_set8",
                vm::op_local_binop64_set8 as crate::common::Op,
            ),
            (
                "op_local_binop64_tee8",
                vm::op_local_binop64_tee8 as crate::common::Op,
            ),
            ("op_local_cmp32", vm::op_local_cmp32 as crate::common::Op),
            (
                "op_local_cmp32_set4",
                vm::op_local_cmp32_set4 as crate::common::Op,
            ),
            (
                "op_local_cmp32_tee4",
                vm::op_local_cmp32_tee4 as crate::common::Op,
            ),
            (
                "op_local_cmp32_br_if",
                vm::op_local_cmp32_br_if as crate::common::Op,
            ),
            ("op_local_cmp64", vm::op_local_cmp64 as crate::common::Op),
            (
                "op_local_cmp64_set4",
                vm::op_local_cmp64_set4 as crate::common::Op,
            ),
            (
                "op_local_cmp64_tee4",
                vm::op_local_cmp64_tee4 as crate::common::Op,
            ),
            (
                "op_local_cmp64_br_if",
                vm::op_local_cmp64_br_if as crate::common::Op,
            ),
            (
                "op_local_unary32",
                vm::op_local_unary32 as crate::common::Op,
            ),
            (
                "op_local_unary32_set4",
                vm::op_local_unary32_set4 as crate::common::Op,
            ),
            (
                "op_local_unary32_tee4",
                vm::op_local_unary32_tee4 as crate::common::Op,
            ),
            (
                "op_local_unary64",
                vm::op_local_unary64 as crate::common::Op,
            ),
            (
                "op_local_unary64_set8",
                vm::op_local_unary64_set8 as crate::common::Op,
            ),
            (
                "op_local_unary64_tee8",
                vm::op_local_unary64_tee8 as crate::common::Op,
            ),
            (
                "op_local_get4_i32_const_add",
                vm::op_local_get4_i32_const_add as crate::common::Op,
            ),
            (
                "op_local_get4_i32_const_add_set4",
                vm::op_local_get4_i32_const_add_set4 as crate::common::Op,
            ),
            (
                "op_local_get4_local_get4_i32_add_set4",
                vm::op_local_get4_local_get4_i32_add_set4 as crate::common::Op,
            ),
            ("op_i32_eq", vm::op_i32_eq as crate::common::Op),
            ("op_i32_eqz", vm::op_i32_eqz as crate::common::Op),
            ("op_br", vm::op_br as crate::common::Op),
            ("op_call", vm::op_call as crate::common::Op),
            ("op_loop", vm::op_loop as crate::common::Op),
            ("op_end", vm::op_end as crate::common::Op),
            (
                "special_block_return",
                vm::special_block_return as crate::common::Op,
            ),
            (
                "special_function_return",
                vm::special_function_return as crate::common::Op,
            ),
            ("op_i32_load", vm::op_i32_load as crate::common::Op),
            ("op_i32_store", vm::op_i32_store as crate::common::Op),
            (
                "op_i32_load_local_base",
                vm::op_i32_load_local_base as crate::common::Op,
            ),
            (
                "op_i32_store_local_base",
                vm::op_i32_store_local_base as crate::common::Op,
            ),
            ("op_i32_load8_u", vm::op_i32_load8_u as crate::common::Op),
            (
                "op_i32_load8_u_local_base",
                vm::op_i32_load8_u_local_base as crate::common::Op,
            ),
            (
                "op_i32_load8_u_indexed_local_base",
                vm::op_i32_load8_u_indexed_local_base as crate::common::Op,
            ),
            ("op_br_if", vm::op_br_if as crate::common::Op),
            (
                "op_local_get4_br_if",
                vm::op_local_get4_br_if as crate::common::Op,
            ),
            (
                "op_local_get4_i32_eqz_br_if",
                vm::op_local_get4_i32_eqz_br_if as crate::common::Op,
            ),
            (
                "op_local_get4_i32_const_compare_br_if",
                vm::op_local_get4_i32_const_compare_br_if as crate::common::Op,
            ),
            ("op_select4", vm::op_select4 as crate::common::Op),
            ("op_select8", vm::op_select8 as crate::common::Op),
            ("op_select16", vm::op_select16 as crate::common::Op),
        ];
        names
            .into_iter()
            .find_map(|(name, candidate)| std::ptr::fn_addr_eq(candidate, op).then_some(name))
            .unwrap_or("other")
    }

    fn debug_decoded_ops(expr: &[Instr]) -> Vec<&'static str> {
        decoded_ops(expr).into_iter().map(debug_op_name).collect()
    }

    fn decoded_starts(expr: &[Instr]) -> Vec<usize> {
        let mut starts = Vec::new();
        let mut cursor = 0usize;
        while cursor < expr.len() {
            let op = unsafe { expr[cursor].op };
            starts.push(cursor);
            cursor += 1 + operand_width(op);
        }
        starts
    }

    fn first_memarg_offset(expr: &[Instr], op: crate::common::Op) -> Option<u32> {
        let mut cursor = 0usize;
        while cursor < expr.len() {
            let current = unsafe { expr[cursor].op };
            if std::ptr::fn_addr_eq(current, op) {
                let memarg_index = memarg_operand_index(op)?;
                return Some(unsafe { expr[cursor + memarg_index].operand.memarg.offset });
            }
            cursor += 1 + operand_width(current);
        }
        None
    }

    fn first_i32_operand(
        expr: &[Instr],
        op: crate::common::Op,
        operand_index: usize,
    ) -> Option<i32> {
        let mut cursor = 0usize;
        while cursor < expr.len() {
            let current = unsafe { expr[cursor].op };
            if std::ptr::fn_addr_eq(current, op) {
                return Some(unsafe { expr[cursor + operand_index].operand.i32 });
            }
            cursor += 1 + operand_width(current);
        }
        None
    }

    fn local_get4_addrs(expr: &[Instr]) -> Vec<u32> {
        let mut cursor = 0usize;
        let mut out = Vec::new();
        while cursor < expr.len() {
            let current = unsafe { expr[cursor].op };
            if std::ptr::fn_addr_eq(current, vm::op_local_get4 as crate::common::Op) {
                out.push(unsafe { expr[cursor + 1].operand.local_addr });
            }
            cursor += 1 + operand_width(current);
        }
        out
    }

    fn local_get16_addrs(expr: &[Instr]) -> Vec<u32> {
        let mut cursor = 0usize;
        let mut out = Vec::new();
        while cursor < expr.len() {
            let current = unsafe { expr[cursor].op };
            if std::ptr::fn_addr_eq(current, vm::op_local_get16 as crate::common::Op) {
                out.push(unsafe { expr[cursor + 1].operand.local_addr });
            }
            cursor += 1 + operand_width(current);
        }
        out
    }

    fn i32_const_values(expr: &[Instr]) -> Vec<i32> {
        let mut cursor = 0usize;
        let mut out = Vec::new();
        while cursor < expr.len() {
            let current = unsafe { expr[cursor].op };
            if std::ptr::fn_addr_eq(current, vm::op_i32_const as crate::common::Op) {
                out.push(unsafe { expr[cursor + 1].operand.i32 });
            }
            cursor += 1 + operand_width(current);
        }
        out
    }

    fn count_local_binop32_kind(
        expr: &[Instr],
        op: crate::common::Op,
        expected_op: LocalBinop32Op,
        expected_shape: LocalFastRhsShape,
    ) -> usize {
        let mut cursor = 0usize;
        let mut count = 0usize;
        while cursor < expr.len() {
            let current = unsafe { expr[cursor].op };
            if std::ptr::fn_addr_eq(current, op)
                && decode_local_binop32_kind(unsafe { expr[cursor + 1].operand.u32 })
                    == Some((expected_op, expected_shape))
            {
                count += 1;
            }
            cursor += 1 + operand_width(current);
        }
        count
    }

    fn count_local_cmp32_kind(
        expr: &[Instr],
        op: crate::common::Op,
        expected_op: LocalCmp32Op,
        expected_shape: LocalFastRhsShape,
    ) -> usize {
        let mut cursor = 0usize;
        let mut count = 0usize;
        while cursor < expr.len() {
            let current = unsafe { expr[cursor].op };
            if std::ptr::fn_addr_eq(current, op)
                && decode_local_cmp32_kind(unsafe { expr[cursor + 1].operand.u32 })
                    == Some((expected_op, expected_shape))
            {
                count += 1;
            }
            cursor += 1 + operand_width(current);
        }
        count
    }

    fn count_local_binop64_kind(
        expr: &[Instr],
        op: crate::common::Op,
        expected_op: LocalBinop64Op,
        expected_shape: LocalFastRhsShape,
    ) -> usize {
        let mut cursor = 0usize;
        let mut count = 0usize;
        while cursor < expr.len() {
            let current = unsafe { expr[cursor].op };
            if std::ptr::fn_addr_eq(current, op)
                && decode_local_binop64_kind(unsafe { expr[cursor + 1].operand.u32 })
                    == Some((expected_op, expected_shape))
            {
                count += 1;
            }
            cursor += 1 + operand_width(current);
        }
        count
    }

    fn count_local_cmp64_kind(
        expr: &[Instr],
        op: crate::common::Op,
        expected_op: LocalCmp64Op,
        expected_shape: LocalFastRhsShape,
    ) -> usize {
        let mut cursor = 0usize;
        let mut count = 0usize;
        while cursor < expr.len() {
            let current = unsafe { expr[cursor].op };
            if std::ptr::fn_addr_eq(current, op)
                && decode_local_cmp64_kind(unsafe { expr[cursor + 1].operand.u32 })
                    == Some((expected_op, expected_shape))
            {
                count += 1;
            }
            cursor += 1 + operand_width(current);
        }
        count
    }

    fn count_local_unary32_kind(
        expr: &[Instr],
        op: crate::common::Op,
        expected_op: LocalUnary32Op,
    ) -> usize {
        let mut cursor = 0usize;
        let mut count = 0usize;
        while cursor < expr.len() {
            let current = unsafe { expr[cursor].op };
            if std::ptr::fn_addr_eq(current, op)
                && decode_local_unary32_kind(unsafe { expr[cursor + 1].operand.u32 })
                    == Some(expected_op)
            {
                count += 1;
            }
            cursor += 1 + operand_width(current);
        }
        count
    }

    fn count_local_unary64_kind(
        expr: &[Instr],
        op: crate::common::Op,
        expected_op: LocalUnary64Op,
    ) -> usize {
        let mut cursor = 0usize;
        let mut count = 0usize;
        while cursor < expr.len() {
            let current = unsafe { expr[cursor].op };
            if std::ptr::fn_addr_eq(current, op)
                && decode_local_unary64_kind(unsafe { expr[cursor + 1].operand.u32 })
                    == Some(expected_op)
            {
                count += 1;
            }
            cursor += 1 + operand_width(current);
        }
        count
    }

    fn local_binop32_local_pairs(
        expr: &[Instr],
        op: crate::common::Op,
        expected_op: LocalBinop32Op,
        expected_shape: LocalFastRhsShape,
    ) -> Vec<(u32, u32)> {
        let mut cursor = 0usize;
        let mut out = Vec::new();
        while cursor < expr.len() {
            let current = unsafe { expr[cursor].op };
            if std::ptr::fn_addr_eq(current, op)
                && decode_local_binop32_kind(unsafe { expr[cursor + 1].operand.u32 })
                    == Some((expected_op, expected_shape))
            {
                out.push((unsafe { expr[cursor + 2].operand.local_addr }, unsafe {
                    expr[cursor + 3].operand.local_addr
                }));
            }
            cursor += 1 + operand_width(current);
        }
        out
    }

    fn memarg_operand_index(op: crate::common::Op) -> Option<usize> {
        if let Some(family) = specialized_memory_family(op) {
            return Some(family.memarg_index() + 1);
        }
        let second = [
            vm::op_i32_load_local as crate::common::Op,
            vm::op_i32_load8_s_local as crate::common::Op,
            vm::op_i32_load8_u_local as crate::common::Op,
            vm::op_i32_load16_s_local as crate::common::Op,
            vm::op_i32_load16_u_local as crate::common::Op,
            vm::op_i64_load_local as crate::common::Op,
            vm::op_i64_load8_s_local as crate::common::Op,
            vm::op_i64_load8_u_local as crate::common::Op,
            vm::op_i64_load16_s_local as crate::common::Op,
            vm::op_i64_load16_u_local as crate::common::Op,
            vm::op_i64_load32_s_local as crate::common::Op,
            vm::op_i64_load32_u_local as crate::common::Op,
            vm::op_f32_load_local as crate::common::Op,
            vm::op_f64_load_local as crate::common::Op,
            vm::op_i32_store_local as crate::common::Op,
            vm::op_i32_store8_local as crate::common::Op,
            vm::op_i32_store16_local as crate::common::Op,
            vm::op_i64_store_local as crate::common::Op,
            vm::op_i64_store8_local as crate::common::Op,
            vm::op_i64_store16_local as crate::common::Op,
            vm::op_i64_store32_local as crate::common::Op,
            vm::op_f32_store_local as crate::common::Op,
            vm::op_f64_store_local as crate::common::Op,
            vm::op_i32_load_indexed_local as crate::common::Op,
            vm::op_i32_load8_s_indexed_local as crate::common::Op,
            vm::op_i32_load8_u_indexed_local as crate::common::Op,
            vm::op_i32_load16_s_indexed_local as crate::common::Op,
            vm::op_i32_load16_u_indexed_local as crate::common::Op,
            vm::op_i64_load_indexed_local as crate::common::Op,
            vm::op_i64_load8_s_indexed_local as crate::common::Op,
            vm::op_i64_load8_u_indexed_local as crate::common::Op,
            vm::op_i64_load16_s_indexed_local as crate::common::Op,
            vm::op_i64_load16_u_indexed_local as crate::common::Op,
            vm::op_i64_load32_s_indexed_local as crate::common::Op,
            vm::op_i64_load32_u_indexed_local as crate::common::Op,
            vm::op_f32_load_indexed_local as crate::common::Op,
            vm::op_f64_load_indexed_local as crate::common::Op,
            vm::op_i32_store_indexed_local as crate::common::Op,
            vm::op_i32_store8_indexed_local as crate::common::Op,
            vm::op_i32_store16_indexed_local as crate::common::Op,
            vm::op_i64_store_indexed_local as crate::common::Op,
            vm::op_i64_store8_indexed_local as crate::common::Op,
            vm::op_i64_store16_indexed_local as crate::common::Op,
            vm::op_i64_store32_indexed_local as crate::common::Op,
            vm::op_f32_store_indexed_local as crate::common::Op,
            vm::op_f64_store_indexed_local as crate::common::Op,
        ];
        if second
            .iter()
            .any(|candidate| std::ptr::fn_addr_eq(*candidate, op))
        {
            return Some(1);
        }
        let fourth = [
            vm::op_i32_load_local_base as crate::common::Op,
            vm::op_i32_load8_s_local_base as crate::common::Op,
            vm::op_i32_load8_u_local_base as crate::common::Op,
            vm::op_i32_load16_s_local_base as crate::common::Op,
            vm::op_i32_load16_u_local_base as crate::common::Op,
            vm::op_i64_load_local_base as crate::common::Op,
            vm::op_i64_load8_s_local_base as crate::common::Op,
            vm::op_i64_load8_u_local_base as crate::common::Op,
            vm::op_i64_load16_s_local_base as crate::common::Op,
            vm::op_i64_load16_u_local_base as crate::common::Op,
            vm::op_i64_load32_s_local_base as crate::common::Op,
            vm::op_i64_load32_u_local_base as crate::common::Op,
            vm::op_f32_load_local_base as crate::common::Op,
            vm::op_f64_load_local_base as crate::common::Op,
            vm::op_i32_store_local_base as crate::common::Op,
            vm::op_i32_store8_local_base as crate::common::Op,
            vm::op_i32_store16_local_base as crate::common::Op,
            vm::op_i64_store_local_base as crate::common::Op,
            vm::op_i64_store8_local_base as crate::common::Op,
            vm::op_i64_store16_local_base as crate::common::Op,
            vm::op_i64_store32_local_base as crate::common::Op,
            vm::op_f32_store_local_base as crate::common::Op,
            vm::op_f64_store_local_base as crate::common::Op,
            vm::op_i32_load_indexed_local_base as crate::common::Op,
            vm::op_i32_load8_s_indexed_local_base as crate::common::Op,
            vm::op_i32_load8_u_indexed_local_base as crate::common::Op,
            vm::op_i32_load16_s_indexed_local_base as crate::common::Op,
            vm::op_i32_load16_u_indexed_local_base as crate::common::Op,
            vm::op_i64_load_indexed_local_base as crate::common::Op,
            vm::op_i64_load8_s_indexed_local_base as crate::common::Op,
            vm::op_i64_load8_u_indexed_local_base as crate::common::Op,
            vm::op_i64_load16_s_indexed_local_base as crate::common::Op,
            vm::op_i64_load16_u_indexed_local_base as crate::common::Op,
            vm::op_i64_load32_s_indexed_local_base as crate::common::Op,
            vm::op_i64_load32_u_indexed_local_base as crate::common::Op,
            vm::op_f32_load_indexed_local_base as crate::common::Op,
            vm::op_f64_load_indexed_local_base as crate::common::Op,
            vm::op_i32_store_indexed_local_base as crate::common::Op,
            vm::op_i32_store8_indexed_local_base as crate::common::Op,
            vm::op_i32_store16_indexed_local_base as crate::common::Op,
            vm::op_i64_store_indexed_local_base as crate::common::Op,
            vm::op_i64_store8_indexed_local_base as crate::common::Op,
            vm::op_i64_store16_indexed_local_base as crate::common::Op,
            vm::op_i64_store32_indexed_local_base as crate::common::Op,
            vm::op_f32_store_indexed_local_base as crate::common::Op,
            vm::op_f64_store_indexed_local_base as crate::common::Op,
        ];
        if fourth
            .iter()
            .any(|candidate| std::ptr::fn_addr_eq(*candidate, op))
        {
            return Some(3);
        }
        None
    }

    fn operand_width(op: crate::common::Op) -> usize {
        if let Some(family) = specialized_memory_family(op) {
            return family.operand_width();
        }
        let one = [
            vm::special_function_return as crate::common::Op,
            vm::special_block_return as crate::common::Op,
            vm::op_i32_const as crate::common::Op,
            vm::op_local_get4 as crate::common::Op,
            vm::op_local_get8 as crate::common::Op,
            vm::op_local_get16 as crate::common::Op,
            vm::op_local_set4 as crate::common::Op,
            vm::op_local_set8 as crate::common::Op,
            vm::op_local_set16 as crate::common::Op,
            vm::op_local_tee4 as crate::common::Op,
            vm::op_local_tee8 as crate::common::Op,
            vm::op_local_tee16 as crate::common::Op,
            vm::op_br as crate::common::Op,
            vm::op_br_if as crate::common::Op,
            vm::op_if as crate::common::Op,
            vm::op_else as crate::common::Op,
            vm::op_loop as crate::common::Op,
            vm::op_return as crate::common::Op,
            vm::op_call as crate::common::Op,
            vm::op_call_import as crate::common::Op,
            vm::op_return_call as crate::common::Op,
            vm::op_return_call_import as crate::common::Op,
            vm::op_drop as crate::common::Op,
            vm::op_select as crate::common::Op,
            vm::op_global_get4 as crate::common::Op,
            vm::op_global_get8 as crate::common::Op,
            vm::op_global_get16 as crate::common::Op,
            vm::op_global_set4 as crate::common::Op,
            vm::op_global_set8 as crate::common::Op,
            vm::op_global_set16 as crate::common::Op,
            vm::simd::i32x4_extract_lane as crate::common::Op,
            vm::simd::i64x2_extract_lane as crate::common::Op,
            vm::simd::f32x4_replace_lane as crate::common::Op,
            vm::simd::f64x2_replace_lane as crate::common::Op,
            vm::op_table_get as crate::common::Op,
            vm::op_table_set as crate::common::Op,
            vm::op_i32_load_local as crate::common::Op,
            vm::op_i32_load8_s_local as crate::common::Op,
            vm::op_i32_load8_u_local as crate::common::Op,
            vm::op_i32_load16_s_local as crate::common::Op,
            vm::op_i32_load16_u_local as crate::common::Op,
            vm::op_i64_load_local as crate::common::Op,
            vm::op_i64_load8_s_local as crate::common::Op,
            vm::op_i64_load8_u_local as crate::common::Op,
            vm::op_i64_load16_s_local as crate::common::Op,
            vm::op_i64_load16_u_local as crate::common::Op,
            vm::op_i64_load32_s_local as crate::common::Op,
            vm::op_i64_load32_u_local as crate::common::Op,
            vm::op_f32_load_local as crate::common::Op,
            vm::op_f64_load_local as crate::common::Op,
            vm::op_i32_store_local as crate::common::Op,
            vm::op_i32_store8_local as crate::common::Op,
            vm::op_i32_store16_local as crate::common::Op,
            vm::op_i64_store_local as crate::common::Op,
            vm::op_i64_store8_local as crate::common::Op,
            vm::op_i64_store16_local as crate::common::Op,
            vm::op_i64_store32_local as crate::common::Op,
            vm::op_f32_store_local as crate::common::Op,
            vm::op_f64_store_local as crate::common::Op,
        ];
        if one
            .iter()
            .any(|candidate| std::ptr::fn_addr_eq(*candidate, op))
        {
            return 1;
        }
        let two = [
            vm::op_local_get4_i32_const_add as crate::common::Op,
            vm::op_local_get4_local_get4_i32_add as crate::common::Op,
            vm::op_local_get4_br_if as crate::common::Op,
            vm::op_local_get4_i32_eqz_br_if as crate::common::Op,
            vm::op_local_unary32 as crate::common::Op,
            vm::op_local_unary64 as crate::common::Op,
            vm::op_call_indirect as crate::common::Op,
            vm::op_return_call_indirect as crate::common::Op,
        ];
        if two
            .iter()
            .any(|candidate| std::ptr::fn_addr_eq(*candidate, op))
        {
            return 2;
        }
        let three = [
            vm::op_local_binop32 as crate::common::Op,
            vm::op_local_binop64 as crate::common::Op,
            vm::op_local_cmp32 as crate::common::Op,
            vm::op_local_cmp64 as crate::common::Op,
            vm::op_i32_load_local_base as crate::common::Op,
            vm::op_i32_load8_s_local_base as crate::common::Op,
            vm::op_i32_load8_u_local_base as crate::common::Op,
            vm::op_i32_load16_s_local_base as crate::common::Op,
            vm::op_i32_load16_u_local_base as crate::common::Op,
            vm::op_i64_load_local_base as crate::common::Op,
            vm::op_i64_load8_s_local_base as crate::common::Op,
            vm::op_i64_load8_u_local_base as crate::common::Op,
            vm::op_i64_load16_s_local_base as crate::common::Op,
            vm::op_i64_load16_u_local_base as crate::common::Op,
            vm::op_i64_load32_s_local_base as crate::common::Op,
            vm::op_i64_load32_u_local_base as crate::common::Op,
            vm::op_f32_load_local_base as crate::common::Op,
            vm::op_f64_load_local_base as crate::common::Op,
            vm::op_i32_store_local_base as crate::common::Op,
            vm::op_i32_store8_local_base as crate::common::Op,
            vm::op_i32_store16_local_base as crate::common::Op,
            vm::op_i64_store_local_base as crate::common::Op,
            vm::op_i64_store8_local_base as crate::common::Op,
            vm::op_i64_store16_local_base as crate::common::Op,
            vm::op_i64_store32_local_base as crate::common::Op,
            vm::op_f32_store_local_base as crate::common::Op,
            vm::op_f64_store_local_base as crate::common::Op,
            vm::op_local_get4_i32_const_add_set4 as crate::common::Op,
            vm::op_local_get4_i32_const_add_tee4 as crate::common::Op,
            vm::op_local_get4_local_get4_i32_add_set4 as crate::common::Op,
            vm::op_local_get4_local_get4_i32_add_tee4 as crate::common::Op,
            vm::op_local_get4_i32_const_add_br_if as crate::common::Op,
            vm::op_local_get4_local_get4_i32_add_br_if as crate::common::Op,
            vm::op_local_unary32_set4 as crate::common::Op,
            vm::op_local_unary32_tee4 as crate::common::Op,
            vm::op_local_unary64_set8 as crate::common::Op,
            vm::op_local_unary64_tee8 as crate::common::Op,
        ];
        if three
            .iter()
            .any(|candidate| std::ptr::fn_addr_eq(*candidate, op))
        {
            return 3;
        }
        let four = [
            vm::op_local_binop32_set4 as crate::common::Op,
            vm::op_local_binop32_tee4 as crate::common::Op,
            vm::op_local_binop32_br_if as crate::common::Op,
            vm::op_local_binop64_set8 as crate::common::Op,
            vm::op_local_binop64_tee8 as crate::common::Op,
            vm::op_local_cmp32_set4 as crate::common::Op,
            vm::op_local_cmp32_tee4 as crate::common::Op,
            vm::op_local_cmp32_br_if as crate::common::Op,
            vm::op_local_cmp64_set4 as crate::common::Op,
            vm::op_local_cmp64_tee4 as crate::common::Op,
            vm::op_local_cmp64_br_if as crate::common::Op,
            vm::op_i32_load_indexed_local_base as crate::common::Op,
            vm::op_i32_load8_s_indexed_local_base as crate::common::Op,
            vm::op_i32_load8_u_indexed_local_base as crate::common::Op,
            vm::op_i32_load16_s_indexed_local_base as crate::common::Op,
            vm::op_i32_load16_u_indexed_local_base as crate::common::Op,
            vm::op_i64_load_indexed_local_base as crate::common::Op,
            vm::op_i64_load8_s_indexed_local_base as crate::common::Op,
            vm::op_i64_load8_u_indexed_local_base as crate::common::Op,
            vm::op_i64_load16_s_indexed_local_base as crate::common::Op,
            vm::op_i64_load16_u_indexed_local_base as crate::common::Op,
            vm::op_i64_load32_s_indexed_local_base as crate::common::Op,
            vm::op_i64_load32_u_indexed_local_base as crate::common::Op,
            vm::op_f32_load_indexed_local_base as crate::common::Op,
            vm::op_f64_load_indexed_local_base as crate::common::Op,
            vm::op_i32_store_indexed_local_base as crate::common::Op,
            vm::op_i32_store8_indexed_local_base as crate::common::Op,
            vm::op_i32_store16_indexed_local_base as crate::common::Op,
            vm::op_i64_store_indexed_local_base as crate::common::Op,
            vm::op_i64_store8_indexed_local_base as crate::common::Op,
            vm::op_i64_store16_indexed_local_base as crate::common::Op,
            vm::op_i64_store32_indexed_local_base as crate::common::Op,
            vm::op_f32_store_indexed_local_base as crate::common::Op,
            vm::op_f64_store_indexed_local_base as crate::common::Op,
            vm::op_local_get4_i32_const_compare_br_if as crate::common::Op,
            vm::op_local_get4_local_get4_compare_br_if as crate::common::Op,
            vm::op_local_get4_i32_const_add_tee4_br_if as crate::common::Op,
        ];
        if four
            .iter()
            .any(|candidate| std::ptr::fn_addr_eq(*candidate, op))
        {
            return 4;
        }
        if std::ptr::fn_addr_eq(op, vm::op_br_table as crate::common::Op) {
            return 3;
        }
        if std::ptr::fn_addr_eq(op, vm::op_end as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_select4 as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_select8 as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_select16 as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_add as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_mul as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_and as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_or as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_xor as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_clz as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_ctz as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_popcnt as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_eqz as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_eq as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_ne as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_lt_s as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_lt_u as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_gt_s as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_gt_u as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_le_s as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_le_u as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_ge_s as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_ge_u as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_sub as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_div_s as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_div_u as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_shl as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_shr_s as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_shr_u as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_rotl as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_rotr as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_clz as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_ctz as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_popcnt as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_eq as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_ne as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_lt_s as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_lt_u as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_gt_s as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_gt_u as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_le_s as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_le_u as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_ge_s as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_ge_u as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_add as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_sub as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_div_s as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_div_u as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_mul as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_and as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_or as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_xor as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_shl as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_shr_s as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_shr_u as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_rotl as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_rotr as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_add as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_sub as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_mul as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_div as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_eq as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_ne as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_lt as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_gt as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_le as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_ge as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_abs as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_neg as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_ceil as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_floor as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_trunc as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_nearest as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_sqrt as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_add as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_sub as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_mul as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_div as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_eq as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_ne as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_lt as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_gt as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_le as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_ge as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_abs as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_neg as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_ceil as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_floor as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_trunc as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_nearest as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f64_sqrt as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_ref_null as crate::common::Op)
        {
            return 0;
        }
        panic!(
            "unsupported op in optimizer test decoder: {:p}",
            op as *const ()
        );
    }

    fn assert_control_targets_align(expr: &[Instr]) {
        let starts = decoded_starts(expr);
        let start_set = starts.iter().copied().collect::<HashSet<_>>();
        for start in starts {
            let op = unsafe { expr[start].op };
            if let Some(target_word) = control_target_word_index(op) {
                let target = unsafe { expr[start + target_word].operand.jump_addr as usize };
                assert!(
                    start_set.contains(&target),
                    "control target {target} is not aligned for op at {start}",
                );
            } else if std::ptr::fn_addr_eq(op, vm::op_br_table as crate::common::Op) {
                let table_len = unsafe { expr[start + 1].operand.u32 as usize };
                for idx in 0..=table_len {
                    let target = unsafe { expr[start + 2 + idx].operand.jump_addr as usize };
                    assert!(
                        start_set.contains(&target),
                        "br_table target {target} is not aligned for op at {start}",
                    );
                }
            }
        }
    }

    fn jump_target_for_start(expr: &[Instr], start: usize) -> Option<usize> {
        let op = unsafe { expr[start].op };
        let target_word = control_target_word_index(op)?;
        Some(unsafe { expr[start + target_word].operand.jump_addr as usize })
    }

    fn control_target_word_index(op: crate::common::Op) -> Option<usize> {
        if std::ptr::fn_addr_eq(op, vm::op_if as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_else as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_br as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_br_if as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_return as crate::common::Op)
        {
            return Some(1);
        }
        if std::ptr::fn_addr_eq(op, vm::op_local_get4_br_if as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_local_get4_i32_eqz_br_if as crate::common::Op)
        {
            return Some(2);
        }
        if std::ptr::fn_addr_eq(
            op,
            vm::op_local_get4_i32_const_add_br_if as crate::common::Op,
        ) || std::ptr::fn_addr_eq(
            op,
            vm::op_local_get4_local_get4_i32_add_br_if as crate::common::Op,
        ) {
            return Some(3);
        }
        if std::ptr::fn_addr_eq(
            op,
            vm::op_local_get4_i32_const_compare_br_if as crate::common::Op,
        ) || std::ptr::fn_addr_eq(
            op,
            vm::op_local_get4_local_get4_compare_br_if as crate::common::Op,
        ) || std::ptr::fn_addr_eq(
            op,
            vm::op_local_get4_i32_const_add_tee4_br_if as crate::common::Op,
        ) || std::ptr::fn_addr_eq(op, vm::op_local_binop32_br_if as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_local_cmp32_br_if as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_local_cmp64_br_if as crate::common::Op)
        {
            return Some(4);
        }
        None
    }

    fn snapshot(types: &[ValType]) -> StackSnapshot {
        StackSnapshot {
            reachable: true,
            types: types.to_vec(),
        }
    }

    #[test]
    fn optimizer_folds_i32_const_add() {
        let expr = function_expr(
            r#"
            (module
              (func (export "f") (result i32)
                i32.const 1
                i32.const 2
                i32.add))
            "#,
        );
        assert!(std::ptr::fn_addr_eq(
            unsafe { expr[0].op },
            vm::op_i32_const as crate::common::Op
        ));
        assert_eq!(unsafe { expr[1].operand.i32 }, 3);
        assert_eq!(count_op(&expr, vm::op_i32_add as crate::common::Op), 0);
    }

    #[test]
    fn optimizer_rewrites_local_set_get_to_tee() {
        let expr = function_expr(
            r#"
            (module
              (func (export "f") (result i32)
                (local i32)
                i32.const 7
                local.set 0
                local.get 0))
            "#,
        );
        assert!(std::ptr::fn_addr_eq(
            unsafe { expr[0].op },
            vm::op_i32_const as crate::common::Op
        ));
        assert!(std::ptr::fn_addr_eq(
            unsafe { expr[2].op },
            vm::op_local_tee4 as crate::common::Op
        ));
    }

    #[test]
    fn optimizer_removes_const_false_br_if() {
        let expr = function_expr(
            r#"
            (module
              (func (export "f") (result i32)
                block
                  i32.const 0
                  br_if 0
                end
                i32.const 7))
            "#,
        );
        assert_eq!(count_op(&expr, vm::op_br_if as crate::common::Op), 0);
    }

    #[test]
    fn optimizer_preserves_call_indirect_as_explicit_op() {
        let expr = function_expr_at(
            r#"
            (module
              (type $t (func (result i32)))
              (table 1 funcref)
              (elem (i32.const 0) $callee)
              (func $callee (result i32)
                i32.const 7)
              (func (export "f") (result i32)
                i32.const 0
                call_indirect (type $t)))
            "#,
            1,
        );
        assert_eq!(
            count_op(&expr, vm::op_call_indirect as crate::common::Op),
            1
        );
        assert_eq!(count_op(&expr, vm::op_i32_const as crate::common::Op), 1);
    }

    #[test]
    fn optimizer_keeps_call_result_drop_producer() {
        let expr = function_expr_at(
            r#"
            (module
              (func $callee (result i32)
                i32.const 7)
              (func (export "f") (result i32)
                call $callee
                drop
                i32.const 3))
            "#,
            1,
        );
        assert_eq!(count_op(&expr, vm::op_call as crate::common::Op), 1);
        assert_eq!(count_op(&expr, vm::op_drop as crate::common::Op), 1);
    }

    #[test]
    fn optimizer_keeps_load_drop_producer() {
        let expr = function_expr(
            r#"
            (module
              (memory 1)
              (func (export "f") (result i32)
                i32.const 0
                i32.load
                drop
                i32.const 3))
            "#,
        );
        assert_eq!(
            count_op(&expr, vm::op_i32_load_local as crate::common::Op),
            1
        );
        assert_eq!(count_op(&expr, vm::op_drop as crate::common::Op), 1);
    }

    #[test]
    fn optimizer_keeps_table_get_drop_producer() {
        let expr = function_expr(
            r#"
            (module
              (table 1 funcref)
              (func (export "f") (result i32)
                i32.const 0
                table.get 0
                drop
                i32.const 3))
            "#,
        );
        assert_eq!(count_op(&expr, vm::op_table_get as crate::common::Op), 1);
        assert_eq!(count_op(&expr, vm::op_drop as crate::common::Op), 1);
    }

    #[test]
    fn optimizer_keeps_call_count_across_if_merge() {
        let expr = function_expr_at(
            r#"
            (module
              (func $callee (param i32) (result i32)
                local.get 0)
              (func (export "f") (param i32) (result i32)
                block
                  local.get 0
                  if
                    i32.const 1
                    call $callee
                    local.set 0
                  else
                    i32.const 2
                    call $callee
                    local.set 0
                  end
                end
                local.get 0))
            "#,
            1,
        );
        assert_eq!(count_op(&expr, vm::op_call as crate::common::Op), 2);
    }

    #[test]
    fn optimizer_reuses_materialized_local_cse_within_block() {
        let expr = function_expr(
            r#"
            (module
              (func (export "f") (param i32 i32) (result i32)
                (local i32)
                local.get 0
                local.get 1
                i32.add
                local.set 2
                local.get 0
                local.get 1
                i32.add
                local.get 2
                i32.add))
            "#,
        );
        assert_eq!(count_i32_add_family(&expr), 2);
    }

    #[test]
    fn optimizer_pre_reuses_slot_backed_value_across_diamond_join() {
        let expr = function_expr(
            r#"
            (module
              (func (export "f") (param i32 i32 i32) (result i32)
                (local i32)
                local.get 0
                if
                  local.get 1
                  local.get 2
                  i32.add
                  local.set 3
                else
                  local.get 1
                  local.get 2
                  i32.add
                  local.set 3
                end
                local.get 1
                local.get 2
                i32.add
                local.get 3
                i32.add))
            "#,
        );
        let ops = debug_decoded_ops(&expr);
        assert_eq!(count_i32_add_family(&expr), 3, "{ops:?}");
    }

    #[test]
    fn optimizer_does_not_cse_across_call_barrier() {
        let expr = function_expr_at(
            r#"
            (module
              (func $touch)
              (func (export "f") (param i32 i32) (result i32)
                (local i32)
                local.get 0
                local.get 1
                i32.add
                local.set 2
                call $touch
                local.get 0
                local.get 1
                i32.add
                local.get 2
                i32.add))
            "#,
            1,
        );
        assert_eq!(count_i32_add_family(&expr), 3);
    }

    #[test]
    fn optimizer_does_not_cse_across_memory_load_barrier() {
        let expr = function_expr(
            r#"
            (module
              (memory 1)
              (func (export "f") (param i32 i32) (result i32)
                (local i32)
                local.get 0
                local.get 1
                i32.add
                local.set 2
                i32.const 0
                i32.load
                drop
                local.get 0
                local.get 1
                i32.add
                local.get 2
                i32.add))
            "#,
        );
        assert_eq!(count_i32_add_family(&expr), 3);
    }

    #[test]
    fn optimizer_does_not_cse_across_memory_store_barrier() {
        let expr = function_expr(
            r#"
            (module
              (memory 1)
              (func (export "f") (param i32 i32) (result i32)
                (local i32)
                local.get 0
                local.get 1
                i32.add
                local.set 2
                i32.const 0
                i32.const 0
                i32.store
                local.get 0
                local.get 1
                i32.add
                local.get 2
                i32.add))
            "#,
        );
        assert_eq!(count_i32_add_family(&expr), 3);
    }

    #[test]
    fn optimizer_does_not_cse_across_global_get_barrier() {
        let expr = function_expr(
            r#"
            (module
              (global $g (mut i32) (i32.const 0))
              (func (export "f") (param i32 i32) (result i32)
                (local i32)
                local.get 0
                local.get 1
                i32.add
                local.set 2
                global.get $g
                drop
                local.get 0
                local.get 1
                i32.add
                local.get 2
                i32.add))
            "#,
        );
        assert_eq!(count_i32_add_family(&expr), 3);
    }

    #[test]
    fn optimizer_does_not_cse_across_global_set_barrier() {
        let expr = function_expr(
            r#"
            (module
              (global $g (mut i32) (i32.const 0))
              (func (export "f") (param i32 i32) (result i32)
                (local i32)
                local.get 0
                local.get 1
                i32.add
                local.set 2
                i32.const 0
                global.set $g
                local.get 0
                local.get 1
                i32.add
                local.get 2
                i32.add))
            "#,
        );
        assert_eq!(count_i32_add_family(&expr), 3);
    }

    #[test]
    fn optimizer_does_not_cse_across_table_get_barrier() {
        let expr = function_expr(
            r#"
            (module
              (table 1 funcref)
              (func (export "f") (param i32 i32) (result i32)
                (local i32)
                local.get 0
                local.get 1
                i32.add
                local.set 2
                i32.const 0
                table.get 0
                drop
                local.get 0
                local.get 1
                i32.add
                local.get 2
                i32.add))
            "#,
        );
        assert_eq!(count_i32_add_family(&expr), 3);
    }

    #[test]
    fn optimizer_does_not_cse_across_table_set_barrier() {
        let expr = function_expr(
            r#"
            (module
              (table 1 funcref)
              (func (export "f") (param i32 i32) (result i32)
                (local i32)
                local.get 0
                local.get 1
                i32.add
                local.set 2
                i32.const 0
                ref.null func
                table.set 0
                local.get 0
                local.get 1
                i32.add
                local.get 2
                i32.add))
            "#,
        );
        assert_eq!(count_i32_add_family(&expr), 3);
    }

    #[test]
    fn optimizer_keeps_loop_jump_targets_on_instruction_boundaries() {
        let expr = function_expr_at(
            r#"
            (module
              (func $step (param i32) (result i32)
                local.get 0
                i32.const 1
                i32.add)
              (func (export "run") (param $remaining i32) (result i32)
                (local $acc i32)
                i32.const 0
                local.set $acc
                block $done
                  loop $loop
                    local.get $remaining
                    i32.eqz
                    br_if $done

                    local.get $acc
                    call $step
                    local.set $acc

                    local.get $remaining
                    i32.const 1
                    i32.sub
                    local.set $remaining
                    br $loop
                  end
                end
                local.get $acc))
            "#,
            1,
        );
        assert_control_targets_align(&expr);
    }

    #[test]
    fn optimizer_propagates_const_local_across_if_merge() {
        let expr = function_expr(
            r#"
            (module
              (func (export "f") (param i32) (result i32)
                block
                  local.get 0
                  if
                    i32.const 7
                    local.set 0
                  else
                    i32.const 7
                    local.set 0
                  end
                end
                local.get 0))
            "#,
        );
        assert_eq!(count_op(&expr, vm::op_local_get4 as crate::common::Op), 2);
    }

    #[test]
    fn optimizer_forwards_store_to_load_within_block() {
        let expr = function_expr(
            r#"
            (module
              (memory 1)
              (func (export "f") (result i32)
                i32.const 0
                i32.const 42
                i32.store
                i32.const 0
                i32.load))
            "#,
        );
        assert!(count_i32_load_family(&expr) <= 1);
    }

    #[test]
    fn optimizer_forwards_store_to_load_across_merge() {
        let expr = function_expr(
            r#"
            (module
              (memory 1)
              (func (export "f") (param i32) (result i32)
                local.get 0
                if
                  i32.const 0
                  i32.const 42
                  i32.store
                else
                  i32.const 0
                  i32.const 42
                  i32.store
                end
                i32.const 0
                i32.load))
            "#,
        );
        assert_eq!(count_op(&expr, vm::op_i32_load as crate::common::Op), 1);
        assert_eq!(count_i32_load_family(&expr), 1);
    }

    #[test]
    fn optimizer_does_not_alias_memory_loads_with_different_offsets() {
        let expr = function_expr(
            r#"
            (module
              (memory 1)
              (data (i32.const 8) "\01")
              (data (i32.const 16) "\02")
              (func (export "f") (result i32)
                (local $base i32)
                i32.const 0
                local.set $base
                local.get $base
                i32.load8_u offset=8
                local.get $base
                i32.load8_u offset=16
                i32.add))
            "#,
        );
        assert_eq!(
            count_i32_load8_u_family(&expr),
            2,
            "same base with different offsets must not be commoned"
        );
    }

    #[test]
    fn optimizer_folds_address_add_into_load_offset() {
        let expr = function_expr(
            r#"
            (module
              (memory 1)
              (func (export "f") (param $base i32) (result i32)
                local.get $base
                i32.const 8
                i32.add
                i32.load8_u))
            "#,
        );
        assert_eq!(count_op(&expr, vm::op_i32_add as crate::common::Op), 0);
        assert_eq!(
            count_op(&expr, vm::op_local_get4_i32_const_add as crate::common::Op),
            0
        );
        assert_eq!(
            count_op(&expr, vm::op_i32_load8_u_local_base as crate::common::Op),
            1
        );
        assert_eq!(
            first_memarg_offset(&expr, vm::op_i32_load8_u_local_base as crate::common::Op),
            Some(0)
        );
        assert_eq!(
            first_i32_operand(&expr, vm::op_i32_load8_u_local_base as crate::common::Op, 2),
            Some(8)
        );
    }

    #[test]
    fn optimizer_folds_address_add_into_store_offset() {
        let expr = function_expr(
            r#"
            (module
              (memory 1)
              (func (export "f") (param $base i32) (param $value i32)
                local.get $base
                i32.const 3
                i32.add
                local.get $value
                i32.store8))
            "#,
        );
        assert_eq!(count_op(&expr, vm::op_i32_add as crate::common::Op), 0);
        assert_eq!(
            count_op(&expr, vm::op_local_get4_i32_const_add as crate::common::Op),
            0
        );
        assert_eq!(
            count_op(&expr, vm::op_i32_store8_local_base as crate::common::Op),
            1
        );
        assert_eq!(
            first_memarg_offset(&expr, vm::op_i32_store8_local_base as crate::common::Op),
            Some(0)
        );
        assert_eq!(
            first_i32_operand(&expr, vm::op_i32_store8_local_base as crate::common::Op, 2),
            Some(3)
        );
    }

    #[test]
    fn optimizer_folds_spill_local_address_add_into_load_offset() {
        let func = function_at(
            r#"
            (module
              (memory 1)
              (data (i32.const 9) "\2a")
              (global $g (mut i32) (i32.const 8))
              (func (export "f") (result i32)
                global.get $g
                drop
                global.get $g
                i32.const 1
                i32.add
                i32.load8_u))
            "#,
            0,
        );
        assert_eq!(
            count_op(&func.expr, vm::op_global_get4 as crate::common::Op),
            1
        );
        assert_eq!(
            count_op(&func.expr, vm::op_local_tee4 as crate::common::Op),
            1
        );
        assert_eq!(count_op(&func.expr, vm::op_i32_add as crate::common::Op), 0);
        assert_eq!(
            count_op(
                &func.expr,
                vm::op_i32_load8_u_local_base as crate::common::Op
            ),
            1
        );
        assert_eq!(
            first_memarg_offset(
                &func.expr,
                vm::op_i32_load8_u_local_base as crate::common::Op
            ),
            Some(0)
        );
        assert_eq!(
            first_i32_operand(
                &func.expr,
                vm::op_i32_load8_u_local_base as crate::common::Op,
                2
            ),
            Some(1)
        );
    }

    #[test]
    fn optimizer_folds_spill_local_address_add_into_store_offset() {
        let func = function_at(
            r#"
            (module
              (memory 1)
              (global $g (mut i32) (i32.const 8))
              (func (export "f") (param $value i32)
                global.get $g
                drop
                global.get $g
                i32.const 1
                i32.add
                local.get $value
                i32.store8))
            "#,
            0,
        );
        assert_eq!(
            count_op(&func.expr, vm::op_global_get4 as crate::common::Op),
            1
        );
        assert_eq!(
            count_op(&func.expr, vm::op_local_tee4 as crate::common::Op),
            1
        );
        assert_eq!(count_op(&func.expr, vm::op_i32_add as crate::common::Op), 0);
        assert_eq!(
            count_op(
                &func.expr,
                vm::op_i32_store8_local_base as crate::common::Op
            ),
            1
        );
        assert_eq!(
            first_memarg_offset(
                &func.expr,
                vm::op_i32_store8_local_base as crate::common::Op
            ),
            Some(0)
        );
        assert_eq!(
            first_i32_operand(
                &func.expr,
                vm::op_i32_store8_local_base as crate::common::Op,
                2
            ),
            Some(1)
        );
    }

    #[test]
    fn optimizer_specializes_store_with_multi_op_value_suffix() {
        let expr = function_expr(
            r#"
            (module
              (memory 1)
              (func (export "f") (param $base i32) (param $lhs i32) (param $rhs i32)
                local.get $base
                local.get $lhs
                local.get $rhs
                i32.add
                i32.store))
            "#,
        );
        assert_eq!(
            count_op(&expr, vm::op_i32_store_local_base as crate::common::Op),
            1
        );
        assert_eq!(count_op(&expr, vm::op_i32_store as crate::common::Op), 0);
        assert_eq!(
            count_op(&expr, vm::op_i32_add as crate::common::Op)
                + count_op(
                    &expr,
                    vm::op_local_get4_local_get4_i32_add as crate::common::Op
                )
                + count_op(
                    &expr,
                    vm::op_local_get4_local_get4_i32_add_set4 as crate::common::Op
                )
                + count_op(
                    &expr,
                    vm::op_local_get4_local_get4_i32_add_tee4 as crate::common::Op
                ),
            1,
            "value suffix must stay materialized while only the address producer is absorbed",
        );
    }

    #[test]
    fn optimizer_specializes_offset_store_with_multi_op_value_suffix() {
        let expr = function_expr(
            r#"
            (module
              (memory 1)
              (func (export "f") (param $base i32) (param $lhs i32) (param $rhs i32)
                local.get $base
                i32.const 5
                i32.add
                local.get $lhs
                local.get $rhs
                i32.add
                i32.store8))
            "#,
        );
        assert_eq!(
            count_op(&expr, vm::op_i32_store8_local_base as crate::common::Op),
            1
        );
        assert_eq!(count_op(&expr, vm::op_i32_store8 as crate::common::Op), 0);
        assert_eq!(
            first_i32_operand(&expr, vm::op_i32_store8_local_base as crate::common::Op, 2),
            Some(5)
        );
        assert_eq!(
            count_op(&expr, vm::op_i32_add as crate::common::Op)
                + count_op(
                    &expr,
                    vm::op_local_get4_local_get4_i32_add as crate::common::Op
                )
                + count_op(
                    &expr,
                    vm::op_local_get4_local_get4_i32_add_set4 as crate::common::Op
                )
                + count_op(
                    &expr,
                    vm::op_local_get4_local_get4_i32_add_tee4 as crate::common::Op
                ),
            1,
            "the address add must fold into the specialized store while the value add remains",
        );
    }

    #[test]
    fn optimizer_specializes_spill_offset_store_with_multi_op_value_suffix() {
        let func = function_at(
            r#"
            (module
              (memory 1)
              (global $g (mut i32) (i32.const 7))
              (func (export "f") (param $lhs i32) (param $rhs i32)
                global.get $g
                drop
                global.get $g
                i32.const 1
                i32.add
                local.get $lhs
                local.get $rhs
                i32.add
                i32.store8))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &func.expr,
                vm::op_i32_store8_local_base as crate::common::Op
            ),
            1
        );
        assert_eq!(
            count_op(&func.expr, vm::op_i32_store8 as crate::common::Op),
            0
        );
        assert_eq!(
            first_i32_operand(
                &func.expr,
                vm::op_i32_store8_local_base as crate::common::Op,
                2
            ),
            Some(1)
        );
        assert_eq!(
            local_get4_addrs(&func.expr),
            vec![0, 4],
            "after absorbing the spill-based address chain, only the param locals may remain as local.get4 inputs",
        );
    }

    #[test]
    fn optimizer_lowers_select_to_typed_fast_path() {
        let expr = function_expr(
            r#"
            (module
              (func (export "f") (param i32) (param i32) (param i32) (result i32)
                local.get 0
                local.get 1
                local.get 2
                select))
            "#,
        );
        assert_eq!(count_op(&expr, vm::op_select as crate::common::Op), 0);
        assert_eq!(count_op(&expr, vm::op_select4 as crate::common::Op), 1);
    }

    #[test]
    fn optimizer_preserves_select_operand_order_with_local_tee_rhs() {
        let expr = function_expr(
            r#"
            (module
              (func (export "f") (param i32 i32) (result i32)
                (select
                  (local.get 0)
                  (local.tee 0 (i32.const 6))
                  (local.get 1)
                )))
            "#,
        );
        assert_eq!(
            debug_decoded_ops(&expr),
            vec![
                "op_local_get4",
                "op_i32_const",
                "op_local_tee4",
                "op_local_get4",
                "op_select4",
                "op_end",
                "special_function_return",
            ]
        );
        assert_eq!(local_get4_addrs(&expr), vec![0, 4]);
    }

    #[test]
    fn optimizer_preserves_select_operand_order_with_local_tee_lhs() {
        let expr = function_expr(
            r#"
            (module
              (func (export "f") (param i32 i32) (result i32)
                (select
                  (local.tee 0 (i32.const 5))
                  (local.get 0)
                  (local.get 1)
                )))
            "#,
        );
        assert_eq!(
            debug_decoded_ops(&expr),
            vec![
                "op_i32_const",
                "op_local_tee4",
                "op_local_get4",
                "op_local_get4",
                "op_select4",
                "op_end",
                "special_function_return",
            ]
        );
        assert_eq!(i32_const_values(&expr), vec![5]);
        assert_eq!(local_get4_addrs(&expr), vec![0, 4]);
    }

    #[test]
    fn optimizer_preserves_select_operand_order_with_local_tee_cond() {
        let expr = function_expr(
            r#"
            (module
              (func (export "f") (param i32) (result i32)
                (select
                  (i32.const 0)
                  (i32.const 1)
                  (local.tee 0 (i32.const 7))
                )))
            "#,
        );
        assert_eq!(
            debug_decoded_ops(&expr),
            vec![
                "op_i32_const",
                "op_i32_const",
                "op_i32_const",
                "op_local_tee4",
                "op_select4",
                "op_end",
                "special_function_return",
            ]
        );
        assert_eq!(i32_const_values(&expr), vec![0, 1, 7]);
    }

    #[test]
    fn optimizer_preserves_float_binary_eval_order() {
        let expr = function_expr(
            r#"
            (module
              (func (export "f") (param f32) (param f32) (param f32) (result f32)
                (f32.add
                  (f32.div (local.get 0) (local.get 1))
                  (f32.div (local.get 2) (local.get 0))
                )))
            "#,
        );
        assert_eq!(
            local_binop32_local_pairs(
                &expr,
                vm::op_local_binop32 as crate::common::Op,
                LocalBinop32Op::F32Div,
                LocalFastRhsShape::Local,
            ),
            vec![(0, 4), (8, 0)]
        );
    }

    #[test]
    fn optimizer_preserves_full_local_tee_select_cond_shape() {
        let module = module_prefix(include_str!(
            "../../../../tests/wasm-testsuite/local_tee.wast"
        ));
        let expr = function_expr_at(&module, 31);
        assert_eq!(
            debug_decoded_ops(&expr),
            vec![
                "op_i32_const",
                "op_i32_const",
                "op_i32_const",
                "op_local_tee4",
                "op_select4",
                "op_end",
                "special_function_return",
            ]
        );
        assert_eq!(i32_const_values(&expr), vec![0, 1, 7]);
    }

    #[test]
    fn optimizer_preserves_binary_right_operand_order_with_local_tee() {
        let expr = function_expr(
            r#"
            (module
              (func (export "f") (param i32) (result i32)
                (i32.sub
                  (i32.const 10)
                  (local.tee 0 (i32.const 4))
                )))
            "#,
        );
        assert_eq!(
            debug_decoded_ops(&expr),
            vec![
                "op_i32_const",
                "op_i32_const",
                "op_local_tee4",
                "op_i32_sub",
                "op_end",
                "special_function_return",
            ]
        );
        assert_eq!(i32_const_values(&expr), vec![10, 4]);
    }

    #[test]
    fn optimizer_preserves_full_local_tee_binary_right_shape() {
        let module = module_prefix(include_str!(
            "../../../../tests/wasm-testsuite/local_tee.wast"
        ));
        let expr = function_expr_at(&module, 51);
        assert_eq!(
            debug_decoded_ops(&expr),
            vec![
                "op_i32_const",
                "op_i32_const",
                "op_local_tee4",
                "op_i32_sub",
                "op_end",
                "special_function_return",
            ]
        );
        assert_eq!(i32_const_values(&expr), vec![10, 4]);
    }

    #[test]
    fn optimizer_preserves_trap_sensitive_drop_binary_ops() {
        let expr = function_expr(
            r#"
            (module
              (func (export "f") (param i64 i64)
                (drop (i64.div_u (local.get 0) (local.get 1)))))
            "#,
        );
        assert_eq!(count_op(&expr, vm::op_i64_div_u as crate::common::Op), 1);
        assert_eq!(count_op(&expr, vm::op_drop as crate::common::Op), 1);
    }

    #[test]
    fn optimizer_preserves_full_traps_module_i64_div_u_under_drop() {
        let expr = function_expr_at(
            r#"
            (module
              (func (export "no_dce.i32.div_s") (param i32) (param i32)
                (drop (i32.div_s (local.get 0) (local.get 1))))
              (func (export "no_dce.i32.div_u") (param i32) (param i32)
                (drop (i32.div_u (local.get 0) (local.get 1))))
              (func (export "no_dce.i64.div_s") (param i64) (param i64)
                (drop (i64.div_s (local.get 0) (local.get 1))))
              (func (export "no_dce.i64.div_u") (param i64) (param i64)
                (drop (i64.div_u (local.get 0) (local.get 1)))))
            "#,
            3,
        );
        assert_eq!(count_op(&expr, vm::op_i64_div_u as crate::common::Op), 1);
        assert_eq!(count_op(&expr, vm::op_drop as crate::common::Op), 1);
    }

    #[test]
    fn optimizer_preserves_i32_div_s_drop_operand_order() {
        let expr = function_expr(
            r#"
            (module
              (func (export "f") (param i32 i32)
                (drop (i32.div_s (local.get 0) (local.get 1)))))
            "#,
        );
        assert_eq!(
            debug_decoded_ops(&expr),
            vec![
                "op_local_get4",
                "op_local_get4",
                "op_i32_div_s",
                "op_drop",
                "op_end",
                "special_function_return",
            ]
        );
        assert_eq!(count_op(&expr, vm::op_i32_div_s as crate::common::Op), 1);
        assert_eq!(count_op(&expr, vm::op_drop as crate::common::Op), 1);
        assert_eq!(local_get4_addrs(&expr), vec![0, 4]);
    }

    #[test]
    fn optimizer_folds_new_pure_numeric_ops_to_const() {
        let expr = function_expr(
            r#"
            (module
              (func (export "f") (result i32)
                i32.const 1
                i32.const 5
                i32.shl
                i32.clz))
            "#,
        );
        assert_eq!(count_op(&expr, vm::op_i32_shl as crate::common::Op), 0);
        assert_eq!(count_op(&expr, vm::op_i32_clz as crate::common::Op), 0);
        assert_eq!(count_op(&expr, vm::op_i32_const as crate::common::Op), 1);
    }

    #[test]
    fn optimizer_folds_i64_pure_numeric_ops_to_const() {
        let expr = function_expr(
            r#"
            (module
              (func (export "f") (result i32)
                i64.const 3
                i64.const 5
                i64.or
                i64.const 7
                i64.and
                i64.const 7
                i64.eq))
            "#,
        );
        assert_eq!(count_op(&expr, vm::op_i64_or as crate::common::Op), 0);
        assert_eq!(count_op(&expr, vm::op_i64_and as crate::common::Op), 0);
        assert_eq!(count_op(&expr, vm::op_i64_eq as crate::common::Op), 0);
        assert_eq!(count_op(&expr, vm::op_i32_const as crate::common::Op), 1);
    }

    #[test]
    fn optimizer_reuses_global_get_from_same_value_global_sets_across_merge() {
        let expr = function_expr(
            r#"
            (module
              (global $g (mut i32) (i32.const 0))
              (func (export "f") (param i32) (result i32)
                local.get 0
                if
                  i32.const 7
                  global.set $g
                else
                  i32.const 7
                  global.set $g
                end
                global.get $g))
            "#,
        );
        assert_eq!(count_op(&expr, vm::op_global_get4 as crate::common::Op), 1);
    }

    #[test]
    fn optimizer_reuses_effect_result_global_get_via_temp_local() {
        let func = function_at(
            r#"
            (module
              (global $g (mut i32) (i32.const 7))
              (func (export "f") (result i32)
                global.get $g
                drop
                global.get $g))
            "#,
            0,
        );
        assert_eq!(
            count_op(&func.expr, vm::op_global_get4 as crate::common::Op),
            1
        );
        assert_eq!(
            count_op(&func.expr, vm::op_local_get4 as crate::common::Op),
            1
        );
        assert_eq!(
            count_op(&func.expr, vm::op_local_tee4 as crate::common::Op),
            1
        );
        assert_eq!(func.locals.byte_size(), 4);
    }

    #[test]
    fn optimizer_reuses_effect_result_memory_load_via_temp_local() {
        let func = function_at(
            r#"
            (module
              (memory 1)
              (data (i32.const 0) "\2a")
              (func (export "f") (result i32)
                i32.const 0
                i32.load8_u
                drop
                i32.const 0
                i32.load8_u))
            "#,
            0,
        );
        assert_eq!(
            count_op(&func.expr, vm::op_i32_load8_u_local as crate::common::Op),
            1
        );
        assert_eq!(
            count_op(&func.expr, vm::op_local_get4 as crate::common::Op),
            1
        );
        assert_eq!(
            count_op(&func.expr, vm::op_local_tee4 as crate::common::Op),
            1
        );
        assert_eq!(func.locals.byte_size(), 4);
    }

    #[test]
    fn optimizer_forwards_store_to_load_from_effect_result_via_temp_local() {
        let func = function_at(
            r#"
            (module
              (memory 1)
              (global $g (mut i32) (i32.const 9))
              (func (export "f") (result i32)
                i32.const 0
                global.get $g
                i32.store
                i32.const 0
                i32.load))
            "#,
            0,
        );
        assert_eq!(
            count_op(&func.expr, vm::op_global_get4 as crate::common::Op),
            1
        );
        assert_eq!(
            count_op(&func.expr, vm::op_i32_store_local as crate::common::Op),
            1
        );
        assert_eq!(
            count_op(&func.expr, vm::op_i32_load_local as crate::common::Op),
            0
        );
        assert_eq!(
            count_op(&func.expr, vm::op_local_get4 as crate::common::Op),
            1
        );
        assert_eq!(
            count_op(&func.expr, vm::op_local_tee4 as crate::common::Op),
            1
        );
    }

    #[test]
    fn optimizer_stops_effect_result_global_get_reuse_across_call_barrier() {
        let expr = function_expr_at(
            r#"
            (module
              (global $g (mut i32) (i32.const 7))
              (func $touch)
              (func (export "f") (result i32)
                global.get $g
                drop
                call $touch
                global.get $g))
            "#,
            1,
        );
        assert_eq!(count_op(&expr, vm::op_global_get4 as crate::common::Op), 2);
    }

    #[test]
    fn optimizer_selects_local_const_add_set_superinstruction() {
        let expr = function_expr(
            r#"
            (module
              (func (export "f") (param i32) (result i32)
                local.get 0
                i32.const 1
                i32.add
                local.set 0
                local.get 0))
            "#,
        );
        assert_eq!(
            count_local_binop32_kind(
                &expr,
                vm::op_local_binop32_set4 as crate::common::Op,
                LocalBinop32Op::I32Add,
                LocalFastRhsShape::Const,
            ) + count_local_binop32_kind(
                &expr,
                vm::op_local_binop32_tee4 as crate::common::Op,
                LocalBinop32Op::I32Add,
                LocalFastRhsShape::Const,
            ),
            1
        );
    }

    #[test]
    fn optimizer_selects_local_const_sub_superinstruction() {
        let expr = function_expr(
            r#"
            (module
              (func (export "f") (param i32) (result i32)
                local.get 0
                i32.const 1
                i32.sub
                local.set 0
                local.get 0))
            "#,
        );
        assert_eq!(
            count_local_binop32_kind(
                &expr,
                vm::op_local_binop32_set4 as crate::common::Op,
                LocalBinop32Op::I32Sub,
                LocalFastRhsShape::Const,
            ) + count_local_binop32_kind(
                &expr,
                vm::op_local_binop32_tee4 as crate::common::Op,
                LocalBinop32Op::I32Sub,
                LocalFastRhsShape::Const,
            ),
            1
        );
        assert_eq!(count_op(&expr, vm::op_i32_sub as crate::common::Op), 0);
    }

    #[test]
    fn optimizer_selects_i64_local_local_binop_family() {
        let expr = function_expr(
            r#"
            (module
              (func (export "f") (param i64 i64) (result i64)
                local.get 0
                local.get 1
                i64.xor
                local.set 0
                local.get 0))
            "#,
        );
        assert_eq!(
            count_local_binop64_kind(
                &expr,
                vm::op_local_binop64_set8 as crate::common::Op,
                LocalBinop64Op::I64Xor,
                LocalFastRhsShape::Local,
            ) + count_local_binop64_kind(
                &expr,
                vm::op_local_binop64_tee8 as crate::common::Op,
                LocalBinop64Op::I64Xor,
                LocalFastRhsShape::Local,
            ),
            1
        );
        assert_eq!(count_op(&expr, vm::op_i64_xor as crate::common::Op), 0);
    }

    #[test]
    fn optimizer_selects_f32_local_const_binop_family() {
        let expr = function_expr(
            r#"
            (module
              (func (export "f") (param f32) (result f32)
                local.get 0
                f32.const 1.5
                f32.mul
                local.set 0
                local.get 0))
            "#,
        );
        assert_eq!(
            count_local_binop32_kind(
                &expr,
                vm::op_local_binop32_set4 as crate::common::Op,
                LocalBinop32Op::F32Mul,
                LocalFastRhsShape::Const,
            ) + count_local_binop32_kind(
                &expr,
                vm::op_local_binop32_tee4 as crate::common::Op,
                LocalBinop32Op::F32Mul,
                LocalFastRhsShape::Const,
            ),
            1
        );
        assert_eq!(count_op(&expr, vm::op_f32_mul as crate::common::Op), 0);
    }

    #[test]
    fn optimizer_selects_f64_compare_br_if_family() {
        let expr = function_expr(
            r#"
            (module
              (func (export "f") (param f64 f64) (result i32)
                block $done
                  local.get 0
                  local.get 1
                  f64.lt
                  br_if $done
                  i32.const 1
                  return
                end
                i32.const 2))
            "#,
        );
        assert_control_targets_align(&expr);
        assert_eq!(
            count_local_cmp64_kind(
                &expr,
                vm::op_local_cmp64_br_if as crate::common::Op,
                LocalCmp64Op::F64Lt,
                LocalFastRhsShape::Local,
            ),
            1
        );
        assert_eq!(count_op(&expr, vm::op_br_if as crate::common::Op), 0);
    }

    #[test]
    fn optimizer_selects_local_unary32_families() {
        let clz_expr = function_expr_at(
            r#"
            (module
              (func (export "clz") (param i32) (result i32)
                local.get 0
                i32.clz
                local.set 0
                local.get 0)
              (func (export "neg") (param f32) (result f32)
                local.get 0
                f32.neg
                local.set 0
                local.get 0))
            "#,
            0,
        );
        let neg_expr = function_expr_at(
            r#"
            (module
              (func (export "clz") (param i32) (result i32)
                local.get 0
                i32.clz
                local.set 0
                local.get 0)
              (func (export "neg") (param f32) (result f32)
                local.get 0
                f32.neg
                local.set 0
                local.get 0))
            "#,
            1,
        );
        assert_eq!(
            count_local_unary32_kind(
                &clz_expr,
                vm::op_local_unary32_set4 as crate::common::Op,
                LocalUnary32Op::I32Clz,
            ) + count_local_unary32_kind(
                &clz_expr,
                vm::op_local_unary32_tee4 as crate::common::Op,
                LocalUnary32Op::I32Clz,
            ),
            1
        );
        assert_eq!(
            count_local_unary32_kind(
                &neg_expr,
                vm::op_local_unary32_set4 as crate::common::Op,
                LocalUnary32Op::F32Neg,
            ) + count_local_unary32_kind(
                &neg_expr,
                vm::op_local_unary32_tee4 as crate::common::Op,
                LocalUnary32Op::F32Neg,
            ),
            1
        );
        assert_eq!(count_op(&clz_expr, vm::op_i32_clz as crate::common::Op), 0);
        assert_eq!(count_op(&neg_expr, vm::op_f32_neg as crate::common::Op), 0);
    }

    #[test]
    fn optimizer_selects_local_unary64_families() {
        let popcnt_expr = function_expr_at(
            r#"
            (module
              (func (export "popcnt") (param i64) (result i64)
                local.get 0
                i64.popcnt
                local.set 0
                local.get 0)
              (func (export "sqrt") (param f64) (result f64)
                local.get 0
                f64.sqrt
                local.set 0
                local.get 0))
            "#,
            0,
        );
        let sqrt_expr = function_expr_at(
            r#"
            (module
              (func (export "popcnt") (param i64) (result i64)
                local.get 0
                i64.popcnt
                local.set 0
                local.get 0)
              (func (export "sqrt") (param f64) (result f64)
                local.get 0
                f64.sqrt
                local.set 0
                local.get 0))
            "#,
            1,
        );
        assert_eq!(
            count_local_unary64_kind(
                &popcnt_expr,
                vm::op_local_unary64_set8 as crate::common::Op,
                LocalUnary64Op::I64Popcnt,
            ) + count_local_unary64_kind(
                &popcnt_expr,
                vm::op_local_unary64_tee8 as crate::common::Op,
                LocalUnary64Op::I64Popcnt,
            ),
            1
        );
        assert_eq!(
            count_local_unary64_kind(
                &sqrt_expr,
                vm::op_local_unary64_set8 as crate::common::Op,
                LocalUnary64Op::F64Sqrt,
            ) + count_local_unary64_kind(
                &sqrt_expr,
                vm::op_local_unary64_tee8 as crate::common::Op,
                LocalUnary64Op::F64Sqrt,
            ),
            1
        );
        assert_eq!(
            count_op(&popcnt_expr, vm::op_i64_popcnt as crate::common::Op),
            0
        );
        assert_eq!(
            count_op(&sqrt_expr, vm::op_f64_sqrt as crate::common::Op),
            0
        );
    }

    #[test]
    fn optimizer_keeps_direct_call_specialization_split() {
        let expr = function_expr_at(
            r#"
            (module
              (import "host" "inc" (func $inc (param i32) (result i32)))
              (func $local (param i32) (result i32)
                local.get 0
                i32.const 1
                i32.add)
              (func (export "run") (param i32) (result i32)
                local.get 0
                call $local
                call $inc))
            "#,
            1,
        );
        assert_eq!(count_op(&expr, vm::op_call as crate::common::Op), 1);
        assert_eq!(count_op(&expr, vm::op_call_import as crate::common::Op), 1);
    }

    #[test]
    fn optimizer_rewrites_self_recursive_call_body_without_bailing_out() {
        let expr = function_expr(
            r#"
            (module
              (func (export "run") (param i32) (result i32)
                local.get 0
                i32.eqz
                if
                  i32.const 0
                  return
                end
                local.get 0
                call 0
                drop
                local.get 0
                i32.const 1
                i32.add))
            "#,
        );
        assert_eq!(count_op(&expr, vm::op_call as crate::common::Op), 1);
        assert_eq!(
            count_local_binop32_kind(
                &expr,
                vm::op_local_binop32 as crate::common::Op,
                LocalBinop32Op::I32Add,
                LocalFastRhsShape::Const,
            ),
            1
        );
    }

    #[test]
    fn optimizer_does_not_cse_across_self_recursive_call_barrier() {
        let expr = function_expr(
            r#"
            (module
              (func (export "run") (param i32) (result i32)
                (local i32)
                local.get 0
                i32.const 1
                i32.add
                local.set 1
                local.get 0
                call 0
                drop
                local.get 0
                i32.const 1
                i32.add
                local.get 1
                i32.add))
            "#,
        );
        assert_eq!(count_i32_add_family(&expr), 3);
    }

    #[test]
    fn optimizer_rewrites_self_recursive_return_call_body_without_bailing_out() {
        let expr = function_expr(
            r#"
            (module
              (func (export "run") (param $n i32) (param $acc i32) (result i32)
                local.get $n
                i32.eqz
                if
                  local.get $acc
                  return
                end
                i32.const 0
                drop
                local.get $n
                i32.const 1
                i32.sub
                local.get $acc
                i32.const 1
                i32.add
                return_call 0))
            "#,
        );
        assert_control_targets_align(&expr);
        assert_eq!(count_op(&expr, vm::op_return_call as crate::common::Op), 1);
        assert_eq!(count_op(&expr, vm::op_drop as crate::common::Op), 0);
    }

    #[test]
    fn optimizer_preserves_recursive_fib_code_shape() {
        let expr = function_expr_at(
            r#"
            (module
              (func (export "run") (param i32) (result i32)
                local.get 0
                call 1)
              (func (param i32) (result i32)
                (local i32 i32 i32 i32)
                local.get 0
                i32.const 2
                i32.lt_s
                if
                  local.get 0
                  return
                end
                local.get 0
                i32.const 1
                i32.sub
                local.tee 4
                call 1
                local.set 1
                local.get 0
                i32.const 2
                i32.sub
                local.tee 3
                call 1
                local.set 2
                local.get 1
                local.get 2
                i32.add))
            "#,
            1,
        );
        assert!(count_i32_add_family(&expr) >= 1);
        assert!(count_op(&expr, vm::op_i32_sub as crate::common::Op) <= 2);
        assert_eq!(count_op(&expr, vm::op_call as crate::common::Op), 2);
    }

    #[test]
    fn build_program_splits_control_boundaries_and_targets() {
        let instrs = vec![
            Instr {
                op: vm::op_i32_const,
            },
            Instr {
                operand: Operand { i32: 1 },
            },
            Instr { op: vm::op_if },
            Instr {
                operand: Operand { jump_addr: 6 },
            },
            Instr {
                op: vm::op_i32_const,
            },
            Instr {
                operand: Operand { i32: 2 },
            },
            Instr { op: vm::op_else },
            Instr {
                operand: Operand { jump_addr: 10 },
            },
            Instr { op: vm::op_loop },
            Instr {
                operand: Operand {
                    loop_param: LoopParam {
                        stack_top: 0,
                        param_size: 0,
                    },
                },
            },
            Instr { op: vm::op_end },
            Instr {
                op: vm::special_function_return,
            },
            Instr {
                operand: Operand { drop_size: 4 },
            },
        ];
        let meta = vec![
            InstructionMeta {
                start: 0,
                len: 2,
                stack_before: snapshot(&[]),
                stack_after: snapshot(&[ValType::I32]),
                preserved_prefix_len: 0,
                fresh_result_count: 1,
            },
            InstructionMeta {
                start: 2,
                len: 2,
                stack_before: snapshot(&[ValType::I32]),
                stack_after: snapshot(&[]),
                preserved_prefix_len: 0,
                fresh_result_count: 0,
            },
            InstructionMeta {
                start: 4,
                len: 2,
                stack_before: snapshot(&[]),
                stack_after: snapshot(&[ValType::I32]),
                preserved_prefix_len: 0,
                fresh_result_count: 1,
            },
            InstructionMeta {
                start: 6,
                len: 2,
                stack_before: snapshot(&[]),
                stack_after: snapshot(&[]),
                preserved_prefix_len: 0,
                fresh_result_count: 0,
            },
            InstructionMeta {
                start: 8,
                len: 2,
                stack_before: snapshot(&[]),
                stack_after: snapshot(&[]),
                preserved_prefix_len: 0,
                fresh_result_count: 0,
            },
            InstructionMeta {
                start: 10,
                len: 1,
                stack_before: snapshot(&[]),
                stack_after: snapshot(&[]),
                preserved_prefix_len: 0,
                fresh_result_count: 0,
            },
            InstructionMeta {
                start: 11,
                len: 2,
                stack_before: snapshot(&[ValType::I32]),
                stack_after: snapshot(&[]),
                preserved_prefix_len: 0,
                fresh_result_count: 0,
            },
        ];
        let program = build_program(&instrs, meta).expect("manual program must build");
        assert_eq!(
            program
                .blocks
                .iter()
                .map(|block| block.start)
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4, 5, 6]
        );
    }

    #[test]
    fn patch_jump_targets_reindexes_br_table() {
        let mut records = vec![
            RecordEmit {
                source_start: Some(0),
                op: vm::op_loop as crate::common::Op,
                operands: vec![Operand {
                    loop_param: crate::common::LoopParam {
                        stack_top: 0,
                        param_size: 0,
                    },
                }],
            },
            RecordEmit {
                source_start: Some(2),
                op: vm::op_br_table as crate::common::Op,
                operands: vec![
                    Operand { u32: 1 },
                    Operand { jump_addr: 0 },
                    Operand { jump_addr: 6 },
                ],
            },
            RecordEmit {
                source_start: Some(6),
                op: vm::op_end as crate::common::Op,
                operands: vec![],
            },
        ];
        patch_jump_targets(&mut records).expect("jump targets must patch");
        assert_eq!(unsafe { records[1].operands[1].jump_addr }, 0);
        assert_eq!(unsafe { records[1].operands[2].jump_addr }, 6);
    }

    #[test]
    fn optimizer_keeps_loop_invariant_pure_expr_scalar_optimized() {
        let func = function_at(
            r#"
            (module
              (func (export "f") (param $n i32) (param $x i32) (result i32)
                (local $acc i32)
                i32.const 0
                drop
                block $done
                  loop $loop
                    local.get $x
                    i32.const 1
                    i32.add
                    local.set $acc
                    local.get $n
                    i32.eqz
                    br_if $done
                    local.get $n
                    i32.const 1
                    i32.sub
                    local.set $n
                    br $loop
                  end
                end
                local.get $acc))
            "#,
            0,
        );
        assert_eq!(count_i32_add_family(&func.expr), 1);
    }

    #[test]
    fn optimizer_licm_hoists_global_get_when_loop_has_no_global_write() {
        let func = function_at(
            r#"
            (module
              (global $g (mut i32) (i32.const 7))
              (func (export "f") (param $n i32) (result i32)
                (local $acc i32)
                i32.const 0
                drop
                block $done
                  loop $loop
                    global.get $g
                    local.set $acc
                    local.get $n
                    i32.eqz
                    br_if $done
                    local.get $n
                    i32.const 1
                    i32.sub
                    local.set $n
                    br $loop
                  end
                end
                local.get $acc))
            "#,
            0,
        );
        assert_eq!(func.locals.byte_size(), 4);
    }

    #[test]
    fn optimizer_licm_does_not_hoist_global_get_across_global_set_in_loop() {
        let func = function_at(
            r#"
            (module
              (global $g (mut i32) (i32.const 7))
              (func (export "f") (param $n i32) (result i32)
                (local $acc i32)
                i32.const 0
                drop
                block $done
                  loop $loop
                    global.get $g
                    local.set $acc
                    i32.const 9
                    global.set $g
                    local.get $n
                    i32.eqz
                    br_if $done
                    local.get $n
                    i32.const 1
                    i32.sub
                    local.set $n
                    br $loop
                  end
                end
                local.get $acc))
            "#,
            0,
        );
        assert_eq!(func.locals.byte_size(), 4);
    }

    #[test]
    fn optimizer_licm_hoists_global_get_address_preparation_into_temp_local() {
        let func = function_at(
            r#"
            (module
              (memory 1)
              (global $g (mut i32) (i32.const 8))
              (func (export "f") (param $n i32) (result i32)
                (local $acc i32)
                i32.const 9
                global.get $g
                i32.const 1
                i32.add
                i32.store8
                block $done
                  loop $loop
                    global.get $g
                    i32.const 1
                    i32.add
                    i32.load8_u
                    local.set $acc
                    local.get $n
                    i32.eqz
                    br_if $done
                    local.get $n
                    i32.const 1
                    i32.sub
                    local.set $n
                    br $loop
                  end
                end
                local.get $acc))
            "#,
            0,
        );
        let ops = debug_decoded_ops(&func.expr);
        assert_eq!(func.locals.byte_size(), 12);
        assert!(
            ops.iter()
                .filter(|op| **op == "op_local_set4" || **op == "op_local_tee4")
                .count()
                >= 2,
            "phase5 must feed the loop load from a temp local after hoisting: {ops:?}"
        );
    }

    #[test]
    fn optimizer_licm_hoists_compare_preparation_to_direct_br_if() {
        let expr = function_expr(
            r#"
            (module
              (global $g (mut i32) (i32.const 7))
              (func (export "f") (param $n i32) (result i32)
                block $done
                  loop $loop
                    global.get $g
                    i32.const 7
                    i32.eq
                    br_if $done
                    local.get $n
                    i32.eqz
                    br_if $done
                    local.get $n
                    i32.const 1
                    i32.sub
                    local.set $n
                    br $loop
                  end
                end
                local.get $n))
            "#,
        );
        assert_control_targets_align(&expr);
        assert_eq!(
            count_op(&expr, vm::op_local_get4_br_if as crate::common::Op),
            1
        );
        assert_eq!(
            count_op(
                &expr,
                vm::op_local_get4_i32_const_compare_br_if as crate::common::Op
            ),
            0
        );
    }

    #[test]
    fn optimizer_keeps_or_eliminates_single_must_alias_load_when_loop_body_is_split_from_loop_header(
    ) {
        let func = function_at(
            r#"
            (module
              (memory 1)
              (func (export "f") (param $n i32) (result i32)
                (local $acc i32)
                i32.const 0
                i32.const 42
                i32.store
                i32.const 0
                drop
                block $done
                  loop $loop
                    i32.const 0
                    i32.load
                    local.set $acc
                    local.get $n
                    i32.eqz
                    br_if $done
                    local.get $n
                    i32.const 1
                    i32.sub
                    local.set $n
                    br $loop
                  end
                end
                local.get $acc))
            "#,
            0,
        );
        assert_eq!(func.locals.byte_size(), 4);
        assert!(
            count_i32_load_family(&func.expr) <= 1,
            "split loop body must not duplicate the must-alias load family"
        );
    }

    #[test]
    fn optimizer_licm_does_not_hoist_load_across_loop_store() {
        let func = function_at(
            r#"
            (module
              (memory 1)
              (func (export "f") (param $n i32) (result i32)
                (local $acc i32)
                i32.const 0
                drop
                block $done
                  loop $loop
                    i32.const 0
                    i32.load
                    local.set $acc
                    i32.const 0
                    i32.const 9
                    i32.store
                    local.get $n
                    i32.eqz
                    br_if $done
                    local.get $n
                    i32.const 1
                    i32.sub
                    local.set $n
                    br $loop
                  end
                end
                local.get $acc))
            "#,
            0,
        );
        assert_eq!(func.locals.byte_size(), 4);
    }

    #[test]
    fn optimizer_selector_skips_multi_use_value_chain() {
        let expr = function_expr(
            r#"
            (module
              (func (export "f") (param i32) (result i32)
                local.get 0
                i32.const 1
                i32.add
                local.tee 0
                local.get 0
                i32.add))
            "#,
        );
        assert_eq!(
            count_op(
                &expr,
                vm::op_local_get4_i32_const_add_tee4 as crate::common::Op
            ),
            0
        );
    }

    #[test]
    fn optimizer_selector_skips_block_argument_value_chain() {
        let expr = function_expr(
            r#"
            (module
              (func (export "f") (param i32) (result i32)
                block
                  local.get 0
                  if
                    i32.const 1
                    local.set 0
                  else
                    i32.const 2
                    local.set 0
                  end
                end
                local.get 0
                i32.const 1
                i32.add
                local.set 0
                local.get 0))
            "#,
        );
        assert_eq!(
            count_op(
                &expr,
                vm::op_local_get4_i32_const_add_set4 as crate::common::Op
            ) + count_op(
                &expr,
                vm::op_local_get4_i32_const_add_tee4 as crate::common::Op
            ) + count_op(&expr, vm::op_local_get4_i32_const_add as crate::common::Op),
            0
        );
    }

    #[test]
    fn optimizer_selector_keeps_lossless_local_shape_across_merge() {
        let expr = function_expr(
            r#"
            (module
              (func (export "f") (param i32 i32) (result i32)
                block
                  local.get 1
                  if
                    local.get 0
                    local.set 0
                  else
                    local.get 0
                    local.set 0
                  end
                end
                local.get 0
                i32.const 1
                i32.add
                local.set 0
                local.get 0))
            "#,
        );
        assert_eq!(
            count_local_binop32_kind(
                &expr,
                vm::op_local_binop32_set4 as crate::common::Op,
                LocalBinop32Op::I32Add,
                LocalFastRhsShape::Const,
            ) + count_local_binop32_kind(
                &expr,
                vm::op_local_binop32_tee4 as crate::common::Op,
                LocalBinop32Op::I32Add,
                LocalFastRhsShape::Const,
            ),
            1
        );
        assert_eq!(
            count_op(&expr, vm::op_local_get4_br_if as crate::common::Op),
            0
        );
        assert_eq!(
            count_op(
                &expr,
                vm::op_local_get4_i32_const_add_br_if as crate::common::Op
            ),
            0
        );
    }

    #[test]
    fn optimizer_selects_direct_local_br_if_family() {
        let expr = function_expr(
            r#"
            (module
              (func (export "f") (param i32) (result i32)
                block $done
                  local.get 0
                  br_if $done
                  i32.const 1
                  return
                end
                i32.const 2))
            "#,
        );
        assert_control_targets_align(&expr);
        assert_eq!(
            count_op(&expr, vm::op_local_get4_br_if as crate::common::Op),
            1
        );
        assert_eq!(count_op(&expr, vm::op_br_if as crate::common::Op), 0);
    }

    #[test]
    fn optimizer_selects_add_and_compare_br_if_families() {
        let add_expr = function_expr(
            r#"
            (module
              (func (export "f") (param i32 i32) (result i32)
                block $done
                  local.get 0
                  i32.const 1
                  i32.add
                  br_if $done
                  local.get 0
                  local.get 1
                  i32.add
                  br_if $done
                  i32.const 1
                  return
                end
                i32.const 2))
            "#,
        );
        assert_control_targets_align(&add_expr);
        assert_eq!(
            count_local_binop32_kind(
                &add_expr,
                vm::op_local_binop32_br_if as crate::common::Op,
                LocalBinop32Op::I32Add,
                LocalFastRhsShape::Const,
            ),
            1
        );
        assert_eq!(
            count_local_binop32_kind(
                &add_expr,
                vm::op_local_binop32_br_if as crate::common::Op,
                LocalBinop32Op::I32Add,
                LocalFastRhsShape::Local,
            ),
            1
        );

        let compare_expr = function_expr(
            r#"
            (module
              (func (export "f") (param i32 i32) (result i32)
                block $done
                  local.get 0
                  i32.eqz
                  br_if $done
                  local.get 0
                  i32.const 7
                  i32.eq
                  br_if $done
                  local.get 0
                  local.get 1
                  i32.lt_s
                  br_if $done
                  i32.const 1
                  return
                end
                i32.const 2))
            "#,
        );
        assert_control_targets_align(&compare_expr);
        assert_eq!(
            count_op(
                &compare_expr,
                vm::op_local_get4_i32_eqz_br_if as crate::common::Op
            ),
            1
        );
        assert_eq!(
            count_local_cmp32_kind(
                &compare_expr,
                vm::op_local_cmp32_br_if as crate::common::Op,
                LocalCmp32Op::I32Eq,
                LocalFastRhsShape::Const,
            ),
            1
        );
        assert_eq!(
            count_local_cmp32_kind(
                &compare_expr,
                vm::op_local_cmp32_br_if as crate::common::Op,
                LocalCmp32Op::I32LtS,
                LocalFastRhsShape::Local,
            ),
            1
        );
    }

    #[test]
    fn optimizer_selects_loop_threading_families_for_memory_loop() {
        let expr = function_expr(
            r#"
            (module
              (memory 1)
              (func (export "run") (param $remaining i32) (result i32)
                i32.const 0
                i32.const 0
                i32.store
                block $done
                  loop $loop
                    local.get $remaining
                    i32.eqz
                    br_if $done

                    i32.const 0
                    i32.const 0
                    i32.load
                    i32.const 1
                    i32.add
                    i32.store

                    local.get $remaining
                    i32.const 1
                    i32.sub
                    local.set $remaining
                    br $loop
                  end
                end
                i32.const 0
                i32.load))
            "#,
        );
        let ops = debug_decoded_ops(&expr);
        let direct_br_if_count =
            count_op(&expr, vm::op_local_get4_i32_eqz_br_if as crate::common::Op)
                + count_op(&expr, vm::op_local_get4_br_if as crate::common::Op);
        assert_control_targets_align(&expr);
        assert_eq!(
            count_op(&expr, vm::op_loop as crate::common::Op),
            1,
            "memory loop must retain an explicit loop header: {ops:?}"
        );
        assert_eq!(
            direct_br_if_count, 1,
            "memory loop should retain a direct local br_if family: {ops:?}"
        );
        assert_eq!(
            count_op(&expr, vm::op_br_if as crate::common::Op),
            0,
            "generic br_if must be eliminated for tail-threading-sensitive loops: {ops:?}"
        );

        let starts = decoded_starts(&expr);
        let loop_start = starts
            .iter()
            .copied()
            .find(|start| {
                std::ptr::fn_addr_eq(unsafe { expr[*start].op }, vm::op_loop as crate::common::Op)
            })
            .expect("memory loop must contain op_loop");
        let br_start = starts
            .iter()
            .copied()
            .find(|start| {
                std::ptr::fn_addr_eq(unsafe { expr[*start].op }, vm::op_br as crate::common::Op)
            })
            .expect("memory loop must contain op_br");
        assert_eq!(
            jump_target_for_start(&expr, br_start),
            Some(loop_start),
            "memory loop backedge must target loop header: {ops:?}"
        );
    }

    #[test]
    fn optimizer_selects_local_base_memory_families_for_loop_invariant_base() {
        let expr = function_expr(
            r#"
            (module
              (memory 1)
              (func (export "run") (param $base i32) (param $remaining i32) (result i32)
                local.get $base
                i32.const 0
                i32.store
                block $done
                  loop $loop
                    local.get $remaining
                    i32.eqz
                    br_if $done

                    local.get $base
                    local.get $base
                    i32.load
                    i32.const 1
                    i32.add
                    i32.store

                    local.get $remaining
                    i32.const 1
                    i32.sub
                    local.set $remaining
                    br $loop
                  end
                end
                local.get $base
                i32.load))
            "#,
        );
        let ops = debug_decoded_ops(&expr);
        let specialized_load = count_op(&expr, vm::op_i32_load_local_base as crate::common::Op);
        let specialized_store = count_op(&expr, vm::op_i32_store_local_base as crate::common::Op);
        let generic_load = count_op(&expr, vm::op_i32_load as crate::common::Op);
        let generic_store = count_op(&expr, vm::op_i32_store as crate::common::Op);

        assert!(
            specialized_load >= 2,
            "loop-invariant load must use local-base families: {ops:?}"
        );
        assert!(
            specialized_store >= 2,
            "loop-invariant store must use local-base families: {ops:?}"
        );
        assert!(
            specialized_load >= generic_load,
            "specialized load families must dominate generic load path: {ops:?}"
        );
        assert!(
            specialized_store >= generic_store,
            "specialized store families must dominate generic store path: {ops:?}"
        );
    }

    #[test]
    fn optimizer_selects_loop_threading_families_for_call_loop() {
        let expr = function_expr_at(
            r#"
            (module
              (func $step (param i32) (result i32)
                local.get 0
                i32.const 1
                i32.add)
              (func (export "run") (param $remaining i32) (result i32)
                (local $acc i32)
                i32.const 0
                local.set $acc
                block $done
                  loop $loop
                    local.get $remaining
                    i32.eqz
                    br_if $done

                    local.get $acc
                    call $step
                    local.set $acc

                    local.get $remaining
                    i32.const 1
                    i32.sub
                    local.set $remaining
                    br $loop
                  end
                end
                local.get $acc))
            "#,
            1,
        );
        let ops = debug_decoded_ops(&expr);
        let direct_br_if_count =
            count_op(&expr, vm::op_local_get4_i32_eqz_br_if as crate::common::Op)
                + count_op(&expr, vm::op_local_get4_br_if as crate::common::Op);
        assert_control_targets_align(&expr);
        assert_eq!(
            count_op(&expr, vm::op_loop as crate::common::Op),
            1,
            "call loop must retain an explicit loop header: {ops:?}"
        );
        assert_eq!(
            direct_br_if_count, 1,
            "call loop should retain a direct local br_if family: {ops:?}"
        );
        assert_eq!(
            count_op(&expr, vm::op_call as crate::common::Op),
            1,
            "call loop should keep the direct local call fast path: {ops:?}"
        );

        let starts = decoded_starts(&expr);
        let loop_start = starts
            .iter()
            .copied()
            .find(|start| {
                std::ptr::fn_addr_eq(unsafe { expr[*start].op }, vm::op_loop as crate::common::Op)
            })
            .expect("call loop must contain op_loop");
        let br_start = starts
            .iter()
            .copied()
            .find(|start| {
                std::ptr::fn_addr_eq(unsafe { expr[*start].op }, vm::op_br as crate::common::Op)
            })
            .expect("call loop must contain op_br");
        assert_eq!(
            jump_target_for_start(&expr, br_start),
            Some(loop_start),
            "call loop backedge must target loop header: {ops:?}"
        );
    }

    #[test]
    fn optimizer_selects_add_tee_br_if_family() {
        let expr = function_expr(
            r#"
            (module
              (func (export "f") (param i32) (result i32)
                (local i32)
                block $done
                  local.get 0
                  i32.const 1
                  i32.add
                  local.tee 1
                  br_if $done
                  i32.const 0
                  return
                end
                local.get 1))
            "#,
        );
        assert_control_targets_align(&expr);
        assert_eq!(
            count_op(
                &expr,
                vm::op_local_get4_i32_const_add_tee4_br_if as crate::common::Op
            ),
            1
        );
        assert_eq!(
            count_op(
                &expr,
                vm::op_local_get4_i32_const_add_tee4 as crate::common::Op
            ),
            0
        );
        assert_eq!(count_op(&expr, vm::op_br_if as crate::common::Op), 0);
    }

    #[test]
    fn optimizer_selects_spill_reused_br_if_families() {
        let global_expr = function_expr(
            r#"
            (module
              (global $g (mut i32) (i32.const 1))
              (func (export "f") (result i32)
                block $done
                  global.get $g
                  drop
                  global.get $g
                  i32.const 1
                  i32.add
                  br_if $done
                  i32.const 11
                  return
                end
                i32.const 22))
            "#,
        );
        assert_control_targets_align(&global_expr);
        assert_eq!(
            count_op(&global_expr, vm::op_global_get4 as crate::common::Op),
            1
        );
        assert_eq!(
            count_op(&global_expr, vm::op_local_tee4 as crate::common::Op),
            1
        );
        assert_eq!(
            count_local_binop32_kind(
                &global_expr,
                vm::op_local_binop32_br_if as crate::common::Op,
                LocalBinop32Op::I32Add,
                LocalFastRhsShape::Const,
            ),
            1
        );
        assert_eq!(count_op(&global_expr, vm::op_br_if as crate::common::Op), 0);

        let load_expr = function_expr(
            r#"
            (module
              (memory 1)
              (data (i32.const 0) "*\00\00\00")
              (func (export "f") (result i32)
                block $done
                  i32.const 0
                  i32.load
                  drop
                  i32.const 0
                  i32.load
                  br_if $done
                  i32.const 11
                  return
                end
                i32.const 22))
            "#,
        );
        assert_control_targets_align(&load_expr);
        assert_eq!(count_i32_load_family(&load_expr), 1);
        assert_eq!(
            count_op(&load_expr, vm::op_local_tee4 as crate::common::Op),
            1
        );
        assert_eq!(
            count_op(&load_expr, vm::op_local_get4_br_if as crate::common::Op),
            1
        );
        assert_eq!(count_op(&load_expr, vm::op_br_if as crate::common::Op), 0);
    }

    #[test]
    fn optimizer_keeps_simd_global_get_live_through_return() {
        let expr = function_expr(
            r#"
            (module
              (global $g (mut v128) (v128.const f32x4 0 0 0 0))
              (func (export "f") (param v128 f32) (result v128)
                (global.set $g (f32x4.replace_lane 0 (local.get 0) (local.get 1)))
                (return (global.get $g))))
            "#,
        );
        let ops = debug_decoded_ops(&expr);
        let local_get16s = local_get16_addrs(&expr);
        assert_control_targets_align(&expr);
        assert!(
            ops.contains(&"f32x4_replace_lane"),
            "expected replace_lane in decoded ops, got {ops:?}"
        );
        assert!(
            ops.contains(&"op_global_set16"),
            "expected global_set16 in decoded ops, got {ops:?}"
        );
        assert!(
            ops.contains(&"op_global_get16"),
            "expected global_get16 in decoded ops, got {ops:?}"
        );
        assert_eq!(
            local_get16s,
            vec![0],
            "unexpected local.get16 addrs for simd global.get reuse: {ops:?}"
        );
        assert_eq!(
            ops,
            vec![
                "op_local_get16",
                "op_local_get4",
                "f32x4_replace_lane",
                "op_global_set16",
                "op_global_get16",
                "op_br",
                "op_end",
                "special_function_return",
            ]
        );
    }
}
