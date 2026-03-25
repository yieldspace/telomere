mod cfg;
mod expr;
mod pass;
mod sink;

use crate::common::{FuncIdx, FuncType, Instr, LocalsData};

pub(crate) use cfg::InstructionMeta;

pub(crate) fn optimize_function(
    funcidx: FuncIdx,
    functype: &FuncType,
    locals: &mut LocalsData,
    instrs: Vec<Instr>,
    meta: Vec<InstructionMeta>,
) -> Vec<Instr> {
    pass::optimize_function(funcidx, functype, locals, instrs, meta)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{
        cfg::{build_program, InstructionMeta},
        pass::patch_jump_targets,
        sink::RecordEmit,
    };
    use crate::{
        common::{Func, FunctionBody, Instr, LoopParam, Operand, ValType},
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

    fn last_non_return_op(expr: &[Instr]) -> crate::common::Op {
        decoded_ops(expr)
            .into_iter()
            .rev()
            .find(|op| {
                !std::ptr::fn_addr_eq(*op, vm::special_function_return as crate::common::Op)
                    && !std::ptr::fn_addr_eq(*op, vm::special_block_return as crate::common::Op)
                    && !std::ptr::fn_addr_eq(*op, vm::op_end as crate::common::Op)
            })
            .expect("expression must contain a non-return op")
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
                return Some(unsafe { expr[cursor + 1].operand.memarg.offset });
            }
            cursor += 1 + operand_width(current);
        }
        None
    }

    fn operand_width(op: crate::common::Op) -> usize {
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
            vm::op_local_get4_i32_const_add_set4 as crate::common::Op,
            vm::op_local_get4_i32_const_add_tee4 as crate::common::Op,
            vm::op_local_get4_local_get4_i32_add_set4 as crate::common::Op,
            vm::op_local_get4_local_get4_i32_add_tee4 as crate::common::Op,
        ];
        if three
            .iter()
            .any(|candidate| std::ptr::fn_addr_eq(*candidate, op))
        {
            return 3;
        }
        if std::ptr::fn_addr_eq(op, vm::op_br_table as crate::common::Op) {
            return 3;
        }
        if std::ptr::fn_addr_eq(op, vm::op_end as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_add as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_clz as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_ctz as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_popcnt as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_eqz as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_ne as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_lt_s as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_ge_u as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i32_sub as crate::common::Op)
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
            || std::ptr::fn_addr_eq(op, vm::op_i64_mul as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_and as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_or as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_xor as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_shl as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_shr_s as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_shr_u as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_rotl as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_i64_rotr as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_abs as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_neg as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_ceil as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_floor as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_trunc as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_nearest as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_f32_sqrt as crate::common::Op)
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
        panic!("unsupported op in optimizer test decoder");
    }

    fn assert_control_targets_align(expr: &[Instr]) {
        let starts = decoded_starts(expr);
        let start_set = starts.iter().copied().collect::<HashSet<_>>();
        for start in starts {
            let op = unsafe { expr[start].op };
            if std::ptr::fn_addr_eq(op, vm::op_if as crate::common::Op)
                || std::ptr::fn_addr_eq(op, vm::op_else as crate::common::Op)
                || std::ptr::fn_addr_eq(op, vm::op_br as crate::common::Op)
                || std::ptr::fn_addr_eq(op, vm::op_br_if as crate::common::Op)
                || std::ptr::fn_addr_eq(op, vm::op_return as crate::common::Op)
            {
                let target = unsafe { expr[start + 1].operand.jump_addr as usize };
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
        assert_eq!(count_op(&expr, vm::op_local_get4 as crate::common::Op), 1);
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
        assert_eq!(
            count_op(&expr, vm::op_i32_load_local as crate::common::Op),
            0
        );
        assert!(!std::ptr::fn_addr_eq(
            last_non_return_op(&expr),
            vm::op_i32_load_local as crate::common::Op
        ));
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
        assert_eq!(
            count_op(&expr, vm::op_i32_load_local as crate::common::Op),
            0
        );
        assert!(!std::ptr::fn_addr_eq(
            last_non_return_op(&expr),
            vm::op_i32_load_local as crate::common::Op
        ));
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
            count_op(&expr, vm::op_i32_load8_u_local as crate::common::Op),
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
            count_op(&expr, vm::op_i32_load8_u_local as crate::common::Op),
            1
        );
        assert_eq!(
            first_memarg_offset(&expr, vm::op_i32_load8_u_local as crate::common::Op),
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
            count_op(&expr, vm::op_i32_store8_local as crate::common::Op),
            1
        );
        assert_eq!(
            first_memarg_offset(&expr, vm::op_i32_store8_local as crate::common::Op),
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
        assert_eq!(
            count_op(&func.expr, vm::op_local_get4 as crate::common::Op),
            1
        );
        assert_eq!(count_op(&func.expr, vm::op_i32_add as crate::common::Op), 0);
        assert_eq!(
            count_op(&func.expr, vm::op_i32_load8_u_local as crate::common::Op),
            1
        );
        assert_eq!(
            first_memarg_offset(&func.expr, vm::op_i32_load8_u_local as crate::common::Op),
            Some(1)
        );
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
        assert_eq!(count_op(&expr, vm::op_global_get4 as crate::common::Op), 0);
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
            count_op(&expr, vm::op_local_get4_i32_const_add as crate::common::Op)
                + count_op(
                    &expr,
                    vm::op_local_get4_i32_const_add_set4 as crate::common::Op
                )
                + count_op(
                    &expr,
                    vm::op_local_get4_i32_const_add_tee4 as crate::common::Op
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
            count_op(&expr, vm::op_local_get4_i32_const_add as crate::common::Op)
                + count_op(
                    &expr,
                    vm::op_local_get4_i32_const_add_set4 as crate::common::Op
                )
                + count_op(
                    &expr,
                    vm::op_local_get4_i32_const_add_tee4 as crate::common::Op
                ),
            1
        );
        assert_eq!(count_op(&expr, vm::op_i32_sub as crate::common::Op), 0);
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
            count_op(&expr, vm::op_local_get4_i32_const_add as crate::common::Op)
                + count_op(
                    &expr,
                    vm::op_local_get4_i32_const_add_set4 as crate::common::Op
                )
                + count_op(
                    &expr,
                    vm::op_local_get4_i32_const_add_tee4 as crate::common::Op
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
        assert!(count_i32_add_family(&expr) >= 2);
        assert!(count_op(&expr, vm::op_i32_sub as crate::common::Op) <= 1);
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
    fn optimizer_keeps_single_must_alias_load_when_loop_body_is_split_from_loop_header() {
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
        assert_eq!(
            count_op(&func.expr, vm::op_i32_load_local as crate::common::Op),
            1
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
}
