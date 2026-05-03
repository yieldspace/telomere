mod cfg;
mod pipeline;

use crate::common::{FuncIdx, FuncType, Instr, LocalsData, LoweredFunction};

pub(crate) use cfg::InstructionMeta;

pub(crate) fn optimize_function(
    funcidx: FuncIdx,
    functype: &FuncType,
    locals: &mut LocalsData,
    instrs: Vec<Instr>,
    meta: Vec<InstructionMeta>,
) -> LoweredFunction {
    pipeline::optimize_function(funcidx, functype, locals, instrs, meta)
}

#[cfg(test)]
mod tests {
    use super::pipeline;
    use crate::{
        common::{ExportDesc, Func, FunctionBody, Instr},
        runtime::vm,
        IoReadBinaryReader, Module, WasmParser,
    };

    fn function_at(wat: &str, func_idx: usize) -> Func {
        let module = module_from_wat(wat);
        let FunctionBody::Wasm(func) = &module.codes.0[func_idx] else {
            panic!("expected wasm function body");
        };
        func.clone()
    }

    fn module_from_wat(wat: &str) -> Module {
        let bytes = wat::parse_str(wat).expect("wat must parse");
        let mut reader = IoReadBinaryReader::from(bytes.as_slice());
        let mut parser = WasmParser::new(&mut reader);
        parser.parse_module().expect("module must parse")
    }

    fn decode_ops(expr: &[Instr], op_lens: &[u16]) -> Vec<crate::common::Op> {
        let mut cursor = 0usize;
        op_lens
            .iter()
            .map(|len| {
                let op = unsafe { expr[cursor].op };
                cursor += usize::from(*len);
                op
            })
            .collect()
    }

    fn count_op(func: &Func, op: crate::common::Op) -> usize {
        decode_ops(&func.expr, &func.op_lens)
            .into_iter()
            .filter(|candidate| std::ptr::fn_addr_eq(*candidate, op))
            .count()
    }

    fn count_lowered_op(func: &Func, op: crate::common::Op) -> usize {
        func.lowered
            .code
            .iter()
            .filter(|candidate| std::ptr::fn_addr_eq(candidate.op, op))
            .count()
    }

    fn first_lowered_op(func: &Func, op: crate::common::Op) -> &crate::common::LoweredOp {
        func.lowered
            .code
            .iter()
            .find(|candidate| std::ptr::fn_addr_eq(candidate.op, op))
            .expect("expected lowered op to exist")
    }

    fn raw_local(local_addr: u32) -> crate::common::LoweredOperand {
        crate::common::LoweredOperand::Raw(unsafe { crate::common::Operand { local_addr }.encoded })
    }

    fn assert_control_targets_align(expr: &[Instr], op_lens: &[u16]) {
        let mut cursor = 0usize;
        for len in op_lens {
            let op = unsafe { expr[cursor].op };
            if std::ptr::fn_addr_eq(op, vm::op_br as crate::common::Op)
                || std::ptr::fn_addr_eq(op, vm::op_br_if as crate::common::Op)
                || std::ptr::fn_addr_eq(op, vm::op_if as crate::common::Op)
                || std::ptr::fn_addr_eq(op, vm::op_else as crate::common::Op)
                || std::ptr::fn_addr_eq(op, vm::op_return as crate::common::Op)
            {
                let target = unsafe { expr[cursor + 1].operand.jump_addr as usize };
                assert!(target < expr.len());
            }
            cursor += usize::from(*len);
        }
    }

    #[test]
    fn optimizer_retains_lowered_artifact() {
        let func = function_at(
            r#"
            (module
              (func (export "run") (param i32) (result i32)
                local.get 0))
            "#,
            0,
        );
        assert!(!func.lowered.code.is_empty());
    }

    #[test]
    fn optimizer_materialize_preserves_expr_stream() {
        let func = function_at(
            r#"
            (module
              (func (export "run") (param i32) (result i32)
                loop $loop
                  local.get 0
                  i32.eqz
                  br_if $loop
                end
                i32.const 7))
            "#,
            0,
        );
        let materialized = func.lowered.materialize();
        assert_eq!(materialized.op_lens, func.op_lens);
        assert_eq!(
            decode_ops(&materialized.instrs, &materialized.op_lens),
            decode_ops(&func.expr, &func.op_lens)
        );
    }

    #[test]
    fn optimizer_materialize_preserves_branch_targets() {
        let func = function_at(
            r#"
            (module
              (func (export "run") (param i32) (result i32)
                loop $loop
                  local.get 0
                  i32.const -1
                  i32.add
                  br_if $loop
                end
                i32.const 7))
            "#,
            0,
        );
        let materialized = func.lowered.materialize();
        assert_control_targets_align(&materialized.instrs, &materialized.op_lens);
        assert_eq!(materialized.op_lens, func.op_lens);
    }

    #[test]
    fn optimizer_temp_slots_start_after_params() {
        let mut locals = crate::common::LocalsData::default();
        locals.set_param_bytes(8);
        assert_eq!(locals.allocate_temp_slot(crate::common::ValType::I32), 8);
        assert_eq!(locals.allocate_temp_slot(crate::common::ValType::I64), 12);
    }

    #[test]
    fn optimizer_selects_br_if_family() {
        let func = function_at(
            r#"
            (module
              (func (export "run") (param i32) (result i32)
                block $done
                  local.get 0
                  i32.eqz
                  br_if $done
                end
                i32.const 1))
            "#,
            0,
        );
        assert_eq!(
            count_op(&func, vm::op_local_get4_i32_eqz_br_if as crate::common::Op),
            1
        );
    }

    #[test]
    fn optimizer_selects_select_width_family() {
        let func = function_at(
            r#"
            (module
              (func (export "run") (param i32 i32 i32) (result i32)
                local.get 0
                local.get 1
                local.get 2
                select))
            "#,
            0,
        );
        assert_eq!(count_op(&func, vm::op_select4 as crate::common::Op), 1);
    }

    #[test]
    fn optimizer_selects_local_get4_run_families() {
        let pair = function_at(
            r#"
            (module
              (func (export "run") (param i32 i32)
                local.get 0
                local.get 1
                drop
                drop))
            "#,
            0,
        );
        assert_eq!(
            count_op(&pair, vm::op_local_get4_local_get4 as crate::common::Op),
            1
        );

        let triple = function_at(
            r#"
            (module
              (func (export "run") (param i32 i32 i32)
                local.get 0
                local.get 1
                local.get 2
                drop
                drop
                drop))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &triple,
                vm::op_local_get4_local_get4_local_get4 as crate::common::Op
            ),
            1
        );

        let longer = function_at(
            r#"
            (module
              (func (export "run") (param i32 i32 i32 i32)
                local.get 0
                local.get 1
                local.get 2
                local.get 3
                drop
                drop
                drop
                drop))
            "#,
            0,
        );
        assert_eq!(
            count_op(&longer, vm::op_local_get4_run as crate::common::Op),
            1
        );
    }

    #[test]
    fn optimizer_selects_stack_i32_const_binop_families() {
        let root = function_at(
            r#"
            (module
              (global $g (mut i32) (i32.const 5))
              (func (export "run") (result i32)
                global.get $g
                i32.const 7
                i32.xor))
            "#,
            0,
        );
        assert_eq!(
            count_op(&root, vm::op_i32_const_binop as crate::common::Op),
            1
        );

        let tee = function_at(
            r#"
            (module
              (global $g (mut i32) (i32.const 8))
              (func (export "run") (result i32)
                (local i32)
                global.get $g
                i32.const 1
                i32.shr_u
                local.tee 0))
            "#,
            0,
        );
        assert_eq!(
            count_op(&tee, vm::op_i32_const_binop_tee4 as crate::common::Op),
            1
        );

        let branch = function_at(
            r#"
            (module
              (global $g (mut i32) (i32.const 1))
              (func (export "run") (result i32)
                block $done
                  global.get $g
                  i32.const 1
                  i32.and
                  br_if $done
                  i32.const 7
                  return
                end
                i32.const 9))
            "#,
            0,
        );
        assert_eq!(
            count_op(&branch, vm::op_i32_const_binop_br_if as crate::common::Op),
            1
        );
    }

    #[test]
    fn optimizer_selects_stack_i32_const_cmp_families() {
        let root = function_at(
            r#"
            (module
              (global $g (mut i32) (i32.const 5))
              (func (export "run") (result i32)
                global.get $g
                i32.const 7
                i32.lt_u))
            "#,
            0,
        );
        assert_eq!(
            count_op(&root, vm::op_i32_const_cmp as crate::common::Op),
            1
        );

        let tee = function_at(
            r#"
            (module
              (global $g (mut i32) (i32.const 8))
              (func (export "run") (result i32)
                (local i32)
                global.get $g
                i32.const 7
                i32.gt_u
                local.tee 0))
            "#,
            0,
        );
        assert_eq!(
            count_op(&tee, vm::op_i32_const_cmp_tee4 as crate::common::Op),
            1
        );

        let branch = function_at(
            r#"
            (module
              (global $g (mut i32) (i32.const 1))
              (func (export "run") (result i32)
                block $done
                  global.get $g
                  i32.const 2
                  i32.lt_u
                  br_if $done
                  i32.const 7
                  return
                end
                i32.const 9))
            "#,
            0,
        );
        assert_eq!(
            count_op(&branch, vm::op_i32_const_cmp_br_if as crate::common::Op),
            1
        );
    }

    #[test]
    fn optimizer_selects_select4_local_consumer_families() {
        let tee = function_at(
            r#"
            (module
              (func (export "run") (param i32 i32 i32) (result i32)
                (local i32)
                local.get 0
                local.get 1
                local.get 2
                select
                local.tee 3))
            "#,
            0,
        );
        assert_eq!(count_op(&tee, vm::op_select4_tee4 as crate::common::Op), 1);

        let set = function_at(
            r#"
            (module
              (func (export "run") (param i32 i32 i32)
                (local i32)
                local.get 0
                local.get 1
                local.get 2
                select
                local.set 3))
            "#,
            0,
        );
        assert_eq!(count_op(&set, vm::op_select4_set4 as crate::common::Op), 1);
    }

    #[test]
    fn optimizer_selects_i32_load16_s_mul_add_local_base_loop() {
        let func = function_at(
            r#"
            (module
              (memory 1)
              (func (export "run") (param i32 i32 i32) (result i32)
                (local i32 i32 i32 i32)
                local.get 0
                local.set 3
                local.get 1
                local.set 4
                local.get 2
                local.set 5
                i32.const 0
                local.set 6
                loop $loop
                  local.get 3
                  i32.load16_s
                  local.get 4
                  i32.load16_s
                  i32.mul
                  local.get 6
                  i32.add
                  local.set 6
                  local.get 3
                  i32.const 2
                  i32.add
                  local.set 3
                  local.get 4
                  i32.const 2
                  i32.add
                  local.set 4
                  local.get 5
                  i32.const -1
                  i32.add
                  local.tee 5
                  br_if $loop
                end
                local.get 6))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &func,
                vm::op_i32_load16_s_mul_add_local_base_loop as crate::common::Op
            ),
            1
        );
    }

    #[test]
    fn optimizer_selects_i32_load16_s_mul_add_local_base_delta_loop() {
        let func = function_at(
            r#"
            (module
              (memory 1)
              (func (export "run") (param i32 i32 i32 i32) (result i32)
                (local i32 i32 i32 i32)
                local.get 0
                local.set 4
                local.get 1
                local.set 5
                local.get 2
                local.set 6
                i32.const 0
                local.set 7
                loop $loop
                  local.get 4
                  i32.load16_s
                  local.get 5
                  i32.load16_s
                  i32.mul
                  local.get 7
                  i32.add
                  local.set 7
                  local.get 5
                  i32.const 2
                  i32.add
                  local.set 5
                  local.get 4
                  local.get 3
                  i32.add
                  local.set 4
                  local.get 6
                  i32.const -1
                  i32.add
                  local.tee 6
                  br_if $loop
                end
                local.get 7))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &func,
                vm::op_i32_load16_s_mul_add_local_base_delta_loop as crate::common::Op
            ),
            1
        );
    }

    #[test]
    fn optimizer_selects_i32_load_store_local_base_local_get4() {
        let func = function_at(
            r#"
            (module
              (memory 1)
              (global $load_addr (mut i32) (i32.const 0))
              (func (export "run") (param i32 i32) (result i32)
                global.get $load_addr
                i32.load
                local.get 1
                local.get 0
                i32.store))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &func,
                vm::op_i32_load_store_local_base_local_get4 as crate::common::Op
            ),
            1
        );
    }

    #[test]
    fn optimizer_selects_i32_load_store_local_base_relink_loop() {
        let func = function_at(
            r#"
            (module
              (memory 1)
              (func (export "run") (param $cursor i32) (param $prev i32)
                (local $current i32)
                loop $again
                  local.get $cursor
                  local.tee $current
                  i32.load
                  local.set $cursor
                  local.get $current
                  local.get $prev
                  i32.store
                  local.get $current
                  local.set $prev
                  local.get $cursor
                  br_if $again
                end))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &func,
                vm::op_i32_load_store_local_base_relink_loop as crate::common::Op
            ),
            1
        );
    }

    #[test]
    fn optimizer_selects_scalar_copy_local_base_run() {
        let func = function_at(
            r#"
            (module
              (memory 1)
              (func (export "run") (param $dst i32) (param $src i32)
                local.get $dst
                local.get $src
                i32.load8_u
                i32.store8
                local.get $dst
                i32.const 1
                i32.add
                local.get $src
                i32.const 1
                i32.add
                i32.load8_u
                i32.store8
                local.get $dst
                i32.const 2
                i32.add
                local.get $src
                i32.const 2
                i32.add
                i32.load8_u
                i32.store8
                local.get $dst
                i32.const 3
                i32.add
                local.get $src
                i32.const 3
                i32.add
                i32.load8_u
                i32.store8))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &func,
                vm::op_scalar_copy_local_base_run as crate::common::Op
            ),
            1
        );
    }

    #[test]
    fn optimizer_rejects_noncontiguous_scalar_copy_run() {
        let func = function_at(
            r#"
            (module
              (memory 1)
              (func (export "run") (param $dst i32) (param $src i32)
                local.get $dst
                local.get $src
                i32.load8_u
                i32.store8
                local.get $dst
                i32.const 2
                i32.add
                local.get $src
                i32.const 2
                i32.add
                i32.load8_u
                i32.store8))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &func,
                vm::op_scalar_copy_local_base_run as crate::common::Op
            ),
            0
        );
    }

    #[test]
    fn optimizer_selects_i32_load_local_base_local_get4_scalar_load_tee4_cmp_br_if() {
        let func = function_at(
            r#"
            (module
              (memory 1)
              (func (export "run") (param i32 i32) (result i32)
                (local i32 i32)
                block $done
                  local.get 0
                  i32.load16_u offset=2
                  local.tee 2
                  local.get 1
                  i32.load16_u
                  local.tee 3
                  i32.eq
                  br_if $done
                  i32.const 7
                  return
                end
                i32.const 9))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &func,
                vm::op_i32_load_local_base_local_get4_i32_load_tee4_cmp_br_if as crate::common::Op
            ),
            1
        );
    }

    #[test]
    fn optimizer_keeps_multi_function_select_memory_bodies_aligned() {
        let module = module_from_wat(
            r#"
            (module
              (memory 1)
              (func (export "as-store-first") (param i32)
                (select (i32.const 0) (i32.const 4) (local.get 0)) (i32.const 1) (i32.store))
              (func (export "as-store-last") (param i32)
                (i32.const 8) (select (i32.const 1) (i32.const 2) (local.get 0)) (i32.store))
              (func (export "as-load-operand") (param i32) (result i32)
                (i32.load (select (i32.const 0) (i32.const 4) (local.get 0))))
              (func (export "load0") (result i32)
                (i32.load (i32.const 0)))
              (func (export "load4") (result i32)
                (i32.load (i32.const 4))))
            "#,
        );

        let func_by_export = |name: &str| -> (u32, Func) {
            let ExportDesc::Func(idx) = module
                .exs
                .find(name)
                .unwrap_or_else(|| panic!("missing export {name}"))
            else {
                panic!("expected function export");
            };
            let FunctionBody::Wasm(func) = &module.codes.0[idx.0 as usize] else {
                panic!("expected wasm function body");
            };
            (idx.0, func.clone())
        };

        assert_eq!(func_by_export("as-store-first").0, 0);
        assert_eq!(func_by_export("as-store-last").0, 1);
        assert_eq!(func_by_export("as-load-operand").0, 2);
        assert_eq!(func_by_export("load0").0, 3);
        assert_eq!(func_by_export("load4").0, 4);

        let (_, store_first) = func_by_export("as-store-first");
        assert_eq!(
            count_lowered_op(&store_first, vm::op_select4 as crate::common::Op),
            1
        );
        assert_eq!(
            count_lowered_op(&store_first, vm::op_i32_store as crate::common::Op),
            1
        );
        assert_eq!(
            count_lowered_op(
                &store_first,
                vm::op_i32_load_const_base as crate::common::Op
            ),
            0
        );

        let (_, load0) = func_by_export("load0");
        assert_eq!(
            count_lowered_op(&load0, vm::op_i32_load_const_base as crate::common::Op),
            1
        );
        assert_eq!(
            count_lowered_op(&load0, vm::op_i32_store as crate::common::Op),
            0
        );
    }

    #[test]
    fn optimizer_selects_select_width_families_for_64_and_128_bit_values() {
        let i64_func = function_at(
            r#"
            (module
              (func (export "run64") (param i64 i64 i32) (result i64)
                local.get 0
                local.get 1
                local.get 2
                select))
            "#,
            0,
        );
        assert_eq!(count_op(&i64_func, vm::op_select8 as crate::common::Op), 1);

        #[cfg(feature = "simd")]
        let v128_func = function_at(
            r#"
            (module
              (func (export "run128") (param v128 v128 i32) (result v128)
                local.get 0
                local.get 1
                local.get 2
                select))
            "#,
            0,
        );
        #[cfg(feature = "simd")]
        assert_eq!(
            count_op(&v128_func, vm::op_select16 as crate::common::Op),
            1
        );
    }

    #[test]
    fn optimizer_selects_const_base_memory_families() {
        let load = function_at(
            r#"
            (module
              (memory 1)
              (data (i32.const 0) "\2a\00\00\00")
              (func (export "load") (result i32)
                i32.const 0
                i32.load))
            "#,
            0,
        );
        assert_eq!(
            count_op(&load, vm::op_i32_load_const_base as crate::common::Op),
            1
        );

        let store = function_at(
            r#"
            (module
              (memory 1)
              (func (export "store") (param i32)
                i32.const 0
                local.get 0
                i32.store))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &store,
                vm::op_i32_store_const_base_local4 as crate::common::Op
            ),
            1
        );
    }

    #[test]
    fn optimizer_selects_full_width_scalar_const_base_memory_families() {
        let i64_load = function_at(
            r#"
            (module
              (memory 1)
              (func (export "load") (result i64)
                i32.const 0
                i64.load))
            "#,
            0,
        );
        assert_eq!(
            count_op(&i64_load, vm::op_i64_load_const_base as crate::common::Op),
            1
        );

        let f32_store = function_at(
            r#"
            (module
              (memory 1)
              (func (export "store") (param f32)
                i32.const 4
                local.get 0
                f32.store))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &f32_store,
                vm::op_f32_store_const_base_local4 as crate::common::Op
            ),
            1
        );

        let f64_store = function_at(
            r#"
            (module
              (memory 1)
              (func (export "store") (param f64)
                i32.const 8
                local.get 0
                f64.store))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &f64_store,
                vm::op_f64_store_const_base_local8 as crate::common::Op
            ),
            1
        );
    }

    #[test]
    fn optimizer_selects_local_base_memory_families() {
        let load = function_at(
            r#"
            (module
              (memory 1)
              (func (export "load") (param i32) (result i32)
                local.get 0
                i32.load))
            "#,
            0,
        );
        assert_eq!(
            count_op(&load, vm::op_i32_load_local_base as crate::common::Op),
            1
        );

        let store = function_at(
            r#"
            (module
              (memory 1)
              (func (export "store") (param i32 i32)
                local.get 0
                local.get 1
                i32.store))
            "#,
            0,
        );
        assert_eq!(
            count_op(&store, vm::op_i32_store_local_base as crate::common::Op),
            0
        );
        assert_eq!(
            count_op(
                &store,
                vm::op_i32_store_local_base_local_get4 as crate::common::Op
            ),
            1
        );
    }

    #[test]
    fn optimizer_selects_full_width_scalar_local_base_memory_families() {
        let i64_load = function_at(
            r#"
            (module
              (memory 1)
              (func (export "load") (param i32) (result i64)
                local.get 0
                i64.load))
            "#,
            0,
        );
        assert_eq!(
            count_op(&i64_load, vm::op_i64_load_local_base as crate::common::Op),
            1
        );

        let f32_store = function_at(
            r#"
            (module
              (memory 1)
              (func (export "store") (param i32 f32)
                local.get 0
                local.get 1
                f32.store))
            "#,
            0,
        );
        assert_eq!(
            count_op(&f32_store, vm::op_f32_store_local_base as crate::common::Op),
            1
        );
    }

    #[test]
    fn optimizer_preserves_memidx_for_indexed_local_base_store_family() {
        let func = function_at(
            r#"
            (module
              (memory 1)
              (memory $m 1)
              (func (export "store") (param i32 i32)
                local.get 0
                local.get 1
                i32.store $m))
            "#,
            0,
        );
        let store = first_lowered_op(
            &func,
            vm::op_i32_store_indexed_local_base as crate::common::Op,
        );
        assert_eq!(store.operands.len(), 4);
        let crate::common::LoweredOperand::Raw(encoded) = store.operands[3] else {
            panic!("expected raw memidx operand");
        };
        let memidx = unsafe { crate::common::Operand { encoded }.u32 };
        assert_eq!(memidx, 1);
    }

    #[test]
    fn optimizer_selects_local_scaled_index_memory_families() {
        let load = function_at(
            r#"
            (module
              (memory 1)
              (func (export "load") (param i32 i32) (result i32)
                local.get 0
                local.get 1
                i32.add
                i32.load))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &load,
                vm::op_i32_load_local_scaled_index as crate::common::Op
            ),
            1
        );

        let store = function_at(
            r#"
            (module
              (memory 1)
              (func (export "store") (param i32 i32 i32)
                local.get 0
                local.get 1
                i32.add
                local.get 2
                i32.store))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &store,
                vm::op_i32_store_local_scaled_index as crate::common::Op
            ),
            1
        );
    }

    #[test]
    fn optimizer_selects_full_width_scalar_local_scaled_index_memory_families() {
        let i64_load = function_at(
            r#"
            (module
              (memory 1)
              (func (export "load") (param i32 i32) (result i64)
                local.get 0
                local.get 1
                i32.const 3
                i32.shl
                i32.add
                i64.load))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &i64_load,
                vm::op_i64_load_local_scaled_index as crate::common::Op
            ),
            1
        );

        let f64_store = function_at(
            r#"
            (module
              (memory 1)
              (func (export "store") (param i32 i32 f64)
                local.get 0
                local.get 1
                i32.const 3
                i32.shl
                i32.add
                local.get 2
                f64.store))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &f64_store,
                vm::op_f64_store_local_scaled_index as crate::common::Op
            ),
            1
        );
    }

    #[test]
    fn optimizer_preserves_memidx_for_indexed_local_scaled_index_store_family() {
        let func = function_at(
            r#"
            (module
              (memory 1)
              (memory $m 1)
              (func (export "store") (param i32 i32 i32)
                local.get 0
                local.get 1
                i32.add
                local.get 2
                i32.store $m))
            "#,
            0,
        );
        let store = first_lowered_op(
            &func,
            vm::op_i32_store_indexed_local_scaled_index as crate::common::Op,
        );
        assert_eq!(store.operands.len(), 6);
        let crate::common::LoweredOperand::Raw(encoded) = store.operands[5] else {
            panic!("expected raw memidx operand");
        };
        let memidx = unsafe { crate::common::Operand { encoded }.u32 };
        assert_eq!(memidx, 1);
    }

    #[test]
    fn optimizer_buffers_same_block_memory_derived_address_root_for_memory_family() {
        let func = function_at(
            r#"
            (module
              (memory 1)
              (data (i32.const 0) "\04\00\00\00\2a\00\00\00")
              (func (export "run") (result i32)
                i32.const 0
                i32.load
                i32.load))
            "#,
            0,
        );
        assert_eq!(
            count_op(&func, vm::op_i32_load_const_base as crate::common::Op),
            1
        );
        assert_eq!(count_op(&func, vm::op_local_set4 as crate::common::Op), 1);
        assert_eq!(count_op(&func, vm::op_local_get4 as crate::common::Op), 0);
        assert_eq!(
            count_op(&func, vm::op_i32_load_local_base as crate::common::Op),
            1
        );
    }

    #[test]
    fn optimizer_selects_local_base_store_family_with_loaded_value_expr() {
        let func = function_at(
            r#"
            (module
              (memory 1)
              (func (export "run") (param $base i32)
                local.get $base
                local.get $base
                i32.load
                i32.const 1
                i32.add
                i32.store))
            "#,
            0,
        );
        assert_eq!(
            count_lowered_op(&func, vm::op_i32_inc_local_base as crate::common::Op),
            1
        );
        assert_eq!(
            count_lowered_op(&func, vm::op_i32_load_local_base as crate::common::Op),
            0
        );
        assert_eq!(
            count_lowered_op(&func, vm::op_i32_store_local_base as crate::common::Op),
            0
        );
    }

    #[test]
    fn optimizer_selects_local_get4_local_base_load_family() {
        let load32 = function_at(
            r#"
            (module
              (memory 1)
              (func (export "run") (param $preserved i32) (param $addr i32) (result i32 i32)
                local.get $preserved
                local.get $addr
                i32.load))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &load32,
                vm::op_local_get4_i32_load_local_base as crate::common::Op
            ),
            1
        );

        let load8 = function_at(
            r#"
            (module
              (memory 1)
              (func (export "run") (param $preserved i32) (param $addr i32) (result i32 i32)
                local.get $preserved
                local.get $addr
                i32.load8_u))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &load8,
                vm::op_local_get4_i32_load8_u_local_base as crate::common::Op
            ),
            1
        );
    }

    #[test]
    fn optimizer_selects_local_base_load_add_set_family() {
        let func = function_at(
            r#"
            (module
              (memory 1)
              (func (export "run") (param $base i32) (param $rhs i32) (result i32)
                (local $dst i32)
                local.get $rhs
                local.get $base
                i32.load
                i32.add
                local.set $dst
                local.get $dst))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &func,
                vm::op_local_get4_i32_load_local_base_i32_add_set4 as crate::common::Op
            ) + count_op(
                &func,
                vm::op_local_get4_i32_load_local_base_i32_add_tee4 as crate::common::Op
            ),
            1
        );
        assert_eq!(count_op(&func, vm::op_i32_add as crate::common::Op), 0);
    }

    #[test]
    fn optimizer_selects_local_base_load_set4_family() {
        let load_set = function_at(
            r#"
            (module
              (memory 1)
              (func (export "run") (param $base i32)
                (local $dst i32)
                local.get $base
                i32.load8_s
                local.set $dst))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &load_set,
                vm::op_i32_load8_s_local_base_set4 as crate::common::Op
            ),
            1
        );

        let load_tee = function_at(
            r#"
            (module
              (memory 1)
              (func (export "run") (param $base i32) (result i32)
                (local $dst i32)
                local.get $base
                i32.load16_u
                local.tee $dst))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &load_tee,
                vm::op_i32_load16_u_local_base_tee4 as crate::common::Op
            ),
            1
        );
    }

    #[test]
    fn optimizer_selects_local_base_load_tee_branch_family() {
        let direct = function_at(
            r#"
            (module
              (memory 1)
              (func (export "run") (param $base i32)
                (local $dst i32)
                block
                  local.get $base
                  i32.load
                  local.tee $dst
                  br_if 0
                end))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &direct,
                vm::op_i32_load_local_base_tee4_br_if as crate::common::Op
            ),
            1
        );

        let eqz = function_at(
            r#"
            (module
              (memory 1)
              (func (export "run") (param $base i32)
                (local $dst i32)
                block
                  local.get $base
                  i32.load
                  local.tee $dst
                  i32.eqz
                  br_if 0
                end))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &eqz,
                vm::op_i32_load_local_base_tee4_i32_eqz_br_if as crate::common::Op
            ),
            1
        );

        let narrow_direct = function_at(
            r#"
            (module
              (memory 1)
              (func (export "run") (param $base i32)
                (local $dst i32)
                block
                  local.get $base
                  i32.load8_u
                  local.tee $dst
                  br_if 0
                end))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &narrow_direct,
                vm::op_i32_load8_u_local_base_tee4_br_if as crate::common::Op
            ),
            1
        );

        let narrow_eqz = function_at(
            r#"
            (module
              (memory 1)
              (func (export "run") (param $base i32)
                (local $dst i32)
                block
                  local.get $base
                  i32.load16_s
                  local.tee $dst
                  i32.eqz
                  br_if 0
                end))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &narrow_eqz,
                vm::op_i32_load16_s_local_base_tee4_i32_eqz_br_if as crate::common::Op
            ),
            1
        );
    }

    #[test]
    fn optimizer_selects_generic_load_tee_branch_family() {
        let direct = function_at(
            r#"
            (module
              (memory 1)
              (global $base i32 (i32.const 8))
              (func (export "run")
                (local $dst i32)
                block
                  global.get $base
                  i32.load8_u
                  local.tee $dst
                  br_if 0
                end))
            "#,
            0,
        );
        assert_eq!(
            count_op(&direct, vm::op_i32_load8_u_tee4_br_if as crate::common::Op),
            1
        );

        let eqz = function_at(
            r#"
            (module
              (memory 1)
              (global $base i32 (i32.const 8))
              (func (export "run")
                (local $dst i32)
                block
                  global.get $base
                  i32.load16_s
                  local.tee $dst
                  i32.eqz
                  br_if 0
                end))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &eqz,
                vm::op_i32_load16_s_tee4_i32_eqz_br_if as crate::common::Op
            ),
            1
        );
    }

    #[test]
    fn optimizer_selects_local_const_and_branch_family() {
        let direct = function_at(
            r#"
            (module
              (func (export "run") (param $value i32)
                block
                  local.get $value
                  i32.const 8
                  i32.and
                  br_if 0
                end))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &direct,
                vm::op_local_get4_i32_const_and_br_if as crate::common::Op
            ),
            1
        );

        let eqz = function_at(
            r#"
            (module
              (func (export "run") (param $value i32)
                block
                  i32.const 8
                  local.get $value
                  i32.and
                  i32.eqz
                  br_if 0
                end))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &eqz,
                vm::op_local_get4_i32_const_and_eqz_br_if as crate::common::Op
            ),
            1
        );
    }

    #[test]
    fn optimizer_selects_local_const_and_tee_const_eq_branch_family() {
        let func = function_at(
            r#"
            (module
              (func (export "run") (param $value i32) (result i32)
                (local $masked i32)
                block
                  local.get $value
                  i32.const 255
                  i32.and
                  local.tee $masked
                  i32.const 44
                  i32.eq
                  br_if 0
                  local.get $masked
                  return
                end
                local.get $masked))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &func,
                vm::op_local_get4_i32_const_and_tee4_i32_const_eq_br_if as crate::common::Op
            ),
            1
        );
    }

    #[test]
    fn optimizer_selects_local_const_and_compare_branch_family() {
        let func = function_at(
            r#"
            (module
              (func (export "run") (param $value i32) (result i32)
                block
                  local.get $value
                  i32.const 223
                  i32.and
                  i32.const 69
                  i32.ne
                  br_if 0
                  i32.const 7
                  return
                end
                i32.const 42))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &func,
                vm::op_local_get4_i32_const_and_i32_const_compare_br_if as crate::common::Op
            ),
            1
        );
    }

    #[test]
    fn optimizer_selects_local_const_add_and_compare_branch_family() {
        let func = function_at(
            r#"
            (module
              (func (export "run") (param $value i32) (result i32)
                block
                  local.get $value
                  i32.const -58
                  i32.add
                  i32.const 255
                  i32.and
                  i32.const 245
                  i32.le_u
                  br_if 0
                  i32.const 7
                  return
                end
                i32.const 42))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &func,
                vm::op_local_get4_i32_const_add_i32_const_and_i32_const_compare_br_if
                    as crate::common::Op
            ),
            1
        );
    }

    #[test]
    fn optimizer_selects_local_copy_const_compare_branch_family() {
        let func = function_at(
            r#"
            (module
              (func (export "run") (param $copy i32) (param $flag i32) (result i32)
                (local $dst i32)
                block
                  local.get $copy
                  local.set $dst
                  local.get $flag
                  i32.const 1
                  i32.ne
                  br_if 0
                  local.get $dst
                  return
                end
                local.get $dst
                i32.const 100
                i32.add))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &func,
                vm::op_local_get4_set4_local_get4_i32_const_compare_br_if as crate::common::Op
            ),
            1
        );
    }

    #[test]
    fn optimizer_fuses_local_add_set_load8_eqz_branch_tail() {
        let func = function_at(
            r#"
            (module
              (memory 1)
              (func (export "run") (param $base i32) (result i32)
                (local $next i32)
                (local $byte i32)
                block
                  local.get $base
                  i32.const 1
                  i32.add
                  local.set $next
                  local.get $base
                  i32.load8_u
                  local.tee $byte
                  i32.eqz
                  br_if 0
                  local.get $next
                  local.get $byte
                  i32.add
                  return
                end
                local.get $next
                local.get $byte
                i32.sub))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &func,
                vm::op_local_get4_i32_const_add_set4_i32_load8_u_local_base_tee4_i32_eqz_br_if
                    as crate::common::Op
            ),
            1
        );
    }

    #[test]
    fn optimizer_fuses_local_base_load_tee_load8_branch_tail() {
        let func = function_at(
            r#"
            (module
              (memory 1)
              (func (export "run") (param $base i32) (result i32)
                (local $ptr i32)
                (local $byte i32)
                block
                  local.get $base
                  i32.load
                  local.tee $ptr
                  i32.load8_u
                  local.tee $byte
                  br_if 0
                  local.get $ptr
                  return
                end
                local.get $byte))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &func,
                vm::op_i32_load_local_base_tee4_i32_load8_u_tee4_br_if as crate::common::Op
            ),
            1
        );
    }

    #[test]
    fn optimizer_selects_local_base_load_local_get4_family() {
        let root = function_at(
            r#"
            (module
              (memory 1)
              (func (export "run") (param $base i32) (param $preserved i32) (result i32 i32)
                local.get $base
                i32.load16_u
                local.get $preserved))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &root,
                vm::op_i32_load16_u_local_base_local_get4 as crate::common::Op
            ),
            1
        );

        let tee = function_at(
            r#"
            (module
              (memory 1)
              (func (export "run") (param $base i32) (param $preserved i32) (result i32 i32)
                (local $dst i32)
                local.get $base
                i32.load8_u
                local.tee $dst
                local.get $preserved))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &tee,
                vm::op_i32_load8_u_local_base_tee4_local_get4 as crate::common::Op
            ),
            1
        );
    }

    #[test]
    fn optimizer_selects_dot4_local_base_loop_family() {
        let func = function_at(
            r#"
            (module
              (memory 1)
              (func (export "run")
                (param $a_base i32) (param $b_base i32) (param $limit i32)
                (result i32)
                (local $idx i32)
                (local $counter i32)
                (local $acc i32)
                (local $a_addr i32)
                (local $b_addr i32)
                loop $again
                  local.get $a_base
                  local.get $idx
                  i32.add
                  local.tee $a_addr
                  i32.const 6
                  i32.add
                  i32.load16_s
                  local.get $b_base
                  local.get $idx
                  i32.add
                  local.tee $b_addr
                  i32.const 6
                  i32.add
                  i32.load16_s
                  i32.mul
                  local.get $a_addr
                  i32.const 4
                  i32.add
                  i32.load16_s
                  local.get $b_addr
                  i32.const 4
                  i32.add
                  i32.load16_s
                  i32.mul
                  local.get $a_addr
                  i32.const 2
                  i32.add
                  i32.load16_s
                  local.get $b_addr
                  i32.const 2
                  i32.add
                  i32.load16_s
                  i32.mul
                  local.get $a_addr
                  i32.load16_s
                  local.get $b_addr
                  i32.load16_s
                  i32.mul
                  local.get $acc
                  i32.add
                  i32.add
                  i32.add
                  i32.add
                  local.set $acc
                  local.get $idx
                  i32.const 8
                  i32.add
                  local.set $idx
                  local.get $limit
                  local.get $counter
                  i32.const 4
                  i32.add
                  local.tee $counter
                  i32.ne
                  br_if $again
                end
                local.get $acc))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &func,
                vm::op_i32_load16_s_dot4_local_base_loop as crate::common::Op
            ),
            1
        );
    }

    #[test]
    fn optimizer_selects_i32_inc_local_base_family() {
        let func = function_at(
            r#"
            (module
              (memory 1)
              (func (export "run") (param $base i32)
                local.get $base
                local.get $base
                i32.load offset=4
                i32.const 1
                i32.add
                i32.store offset=4))
            "#,
            0,
        );
        assert_eq!(
            count_op(&func, vm::op_i32_inc_local_base as crate::common::Op),
            1
        );
    }

    #[test]
    fn optimizer_selects_generic_load_local_get4_family() {
        let root = function_at(
            r#"
            (module
              (memory 1)
              (global $base i32 (i32.const 8))
              (func (export "run") (param $preserved i32) (result i32 i32)
                global.get $base
                i32.load16_s
                local.get $preserved))
            "#,
            0,
        );
        assert_eq!(
            count_op(&root, vm::op_i32_load16_s_local_get4 as crate::common::Op),
            1
        );
    }

    #[test]
    fn optimizer_selects_narrow_local_base_store_local_get4_family() {
        let store8 = function_at(
            r#"
            (module
              (memory 1)
              (func (export "run") (param $base i32) (param $value i32)
                local.get $base
                local.get $value
                i32.store8))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &store8,
                vm::op_i32_store8_local_base_local_get4 as crate::common::Op
            ),
            1
        );

        let store16 = function_at(
            r#"
            (module
              (memory 1)
              (func (export "run") (param $base i32) (param $value i32)
                local.get $base
                local.get $value
                i32.store16))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &store16,
                vm::op_i32_store16_local_base_local_get4 as crate::common::Op
            ),
            1
        );
    }

    #[test]
    fn optimizer_selects_local_get4_copy_family() {
        let set = function_at(
            r#"
            (module
              (func (export "run") (param i32)
                (local i32)
                local.get 0
                local.set 1))
            "#,
            0,
        );
        assert_eq!(
            count_op(&set, vm::op_local_get4_set4 as crate::common::Op),
            1
        );

        let tee = function_at(
            r#"
            (module
              (func (export "run") (param i32) (result i32)
                (local i32)
                local.get 0
                local.tee 1))
            "#,
            0,
        );
        assert_eq!(
            count_op(&tee, vm::op_local_get4_tee4 as crate::common::Op),
            1
        );
    }

    #[test]
    fn optimizer_selects_i32_const_copy_family() {
        let set = function_at(
            r#"
            (module
              (func (export "run")
                (local i32)
                i32.const 7
                local.set 0))
            "#,
            0,
        );
        assert_eq!(
            count_op(&set, vm::op_i32_const_set4 as crate::common::Op),
            1
        );

        let tee = function_at(
            r#"
            (module
              (func (export "run") (result i32)
                (local i32)
                i32.const 7
                local.tee 0))
            "#,
            0,
        );
        assert_eq!(
            count_op(&tee, vm::op_i32_const_tee4 as crate::common::Op),
            1
        );
    }

    #[test]
    fn optimizer_selects_local_unary_family() {
        let func = function_at(
            r#"
            (module
              (func (export "run") (param i32) (result i32)
                local.get 0
                i32.popcnt))
            "#,
            0,
        );
        assert_eq!(
            count_op(&func, vm::op_local_unary32 as crate::common::Op),
            1
        );
    }

    #[test]
    fn optimizer_selects_local_binop_set_family() {
        let func = function_at(
            r#"
            (module
              (func (export "run") (param i32) (result i32)
                (local i32)
                local.get 0
                i32.const 1
                i32.mul
                local.set 1
                i32.const 7))
            "#,
            0,
        );
        assert_eq!(
            count_op(&func, vm::op_local_binop32_set4 as crate::common::Op),
            1
        );
    }

    #[test]
    fn optimizer_selects_local_const_add_set_family() {
        let func = function_at(
            r#"
            (module
              (func (export "run") (param i32) (result i32)
                (local i32)
                local.get 0
                i32.const 1
                i32.add
                local.set 1
                i32.const 7))
            "#,
            0,
        );
        assert!(
            count_op(
                &func,
                vm::op_local_get4_i32_const_add_set4 as crate::common::Op
            ) == 1
        );
    }

    #[test]
    fn optimizer_caches_redundant_local_expression_into_temp_local() {
        let func = function_at(
            r#"
            (module
              (func (export "run") (param i32) (result i32)
                local.get 0
                i32.const 1
                i32.add
                drop
                local.get 0
                i32.const 1
                i32.add))
            "#,
            0,
        );
        assert!(func.local_size() >= 4);
        assert_eq!(
            count_op(
                &func,
                vm::op_local_get4_i32_const_add_tee4 as crate::common::Op
            ),
            1
        );
        assert_eq!(count_op(&func, vm::op_i32_add as crate::common::Op), 0);
    }

    #[test]
    fn optimizer_caches_redundant_const_base_load_into_temp_local() {
        let func = function_at(
            r#"
            (module
              (memory 1)
              (data (i32.const 0) "\2a\00\00\00")
              (func (export "run") (result i32)
                i32.const 0
                i32.load
                drop
                i32.const 0
                i32.load))
            "#,
            0,
        );
        assert!(func.local_size() > 0);
        assert_eq!(
            count_op(&func, vm::op_i32_load_const_base as crate::common::Op),
            1
        );
        assert!(count_op(&func, vm::op_local_get4 as crate::common::Op) >= 1);
    }

    #[test]
    fn optimizer_coalesces_local_set_get_into_tee_family() {
        let func = function_at(
            r#"
            (module
              (func (export "run") (param i32) (result i32)
                (local i32)
                local.get 0
                i32.popcnt
                local.set 1
                local.get 1))
            "#,
            0,
        );
        assert_eq!(
            count_op(&func, vm::op_local_unary32_tee4 as crate::common::Op),
            1
        );
    }

    #[test]
    fn optimizer_selects_local_cmp64_br_if_family() {
        let func = function_at(
            r#"
            (module
              (func (export "run") (param i64 i64) (result i32)
                block $done
                  local.get 0
                  local.get 1
                  i64.lt_s
                  br_if $done
                end
                i32.const 1))
            "#,
            0,
        );
        assert_eq!(
            count_op(&func, vm::op_local_cmp64_br_if as crate::common::Op),
            1
        );
    }

    #[test]
    fn optimizer_selects_local_i32_const_compare_br_if_family() {
        let func = function_at(
            r#"
            (module
              (func (export "run") (param i32) (result i32)
                block $done
                  local.get 0
                  i32.const 7
                  i32.lt_s
                  br_if $done
                end
                i32.const 1))
            "#,
            0,
        );
        assert_eq!(
            count_op(
                &func,
                vm::op_local_get4_i32_const_compare_br_if as crate::common::Op
            ),
            1
        );
    }

    #[test]
    fn optimizer_populates_const_pool() {
        let func = function_at(
            r#"
            (module
              (func (export "run") (result i32)
                i32.const 42
                drop
                i32.const 42))
            "#,
            0,
        );
        assert_eq!(func.lowered.const_pool.len(), 1);
        assert!(func.lowered.code.iter().any(|op| {
            op.operands
                .iter()
                .any(|operand| matches!(operand, crate::common::LoweredOperand::ConstPoolRef(_)))
        }));
    }

    #[test]
    fn optimizer_folds_const_numeric_branch_to_direct_branch() {
        let func = function_at(
            r#"
            (module
              (func (export "run") (result i32)
                block $done
                  i32.const 4
                  i32.const 1
                  i32.sub
                  br_if $done
                end
                i32.const 7))
            "#,
            0,
        );
        assert_eq!(count_op(&func, vm::op_br as crate::common::Op), 1);
        assert_eq!(count_op(&func, vm::op_i32_sub as crate::common::Op), 0);
    }

    #[test]
    fn optimizer_hoists_loop_invariant_expression_into_preheader() {
        let func = function_at(
            r#"
            (module
              (func (export "run") (param $remaining i32) (param $base i32) (result i32)
                (local $acc i32)
                i32.const 0
                local.set $acc
                block $done
                  loop $loop
                    local.get $base
                    i32.const 4
                    i32.add
                    drop
                    local.get $remaining
                    i32.eqz
                    br_if $done
                    local.get $acc
                    i32.const 1
                    i32.add
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
            0,
        );
        assert!(func.local_size() > 4);
        assert!(
            count_op(
                &func,
                vm::op_local_get4_i32_const_add_set4 as crate::common::Op
            ) >= 2
        );
    }

    #[test]
    fn optimizer_versioning_caps_specialized_clones() {
        use crate::common::ValType;
        use pipeline::{
            analysis,
            ir::{
                BlockParam, CanonBlock, CanonFunc, CanonInst, EffectId, InstId, StorageClass,
                ValueId,
            },
            select::{self, BlockVersionKind},
            versioning,
        };

        let block0 = CanonBlock {
            id: 0,
            params: Vec::new(),
            insts: vec![
                CanonInst {
                    id: InstId(0),
                    op: vm::op_local_get4 as crate::common::Op,
                    operands: vec![raw_local(0)],
                    stack_before: Vec::new(),
                    stack_after: vec![ValType::I32],
                    preserved_prefix_len: 0,
                    fresh_result_count: 1,
                    effect: EffectId(0),
                },
                CanonInst {
                    id: InstId(1),
                    op: vm::op_br_if as crate::common::Op,
                    operands: vec![crate::common::LoweredOperand::JumpTarget(1)],
                    stack_before: vec![ValType::I32],
                    stack_after: Vec::new(),
                    preserved_prefix_len: 0,
                    fresh_result_count: 0,
                    effect: EffectId(1),
                },
            ],
            predecessors: Vec::new(),
            successors: vec![1, 2],
        };
        let block1 = CanonBlock {
            id: 1,
            params: vec![BlockParam {
                id: ValueId(0),
                index: 0,
                ty: ValType::I32,
                storage: StorageClass::BlockParam,
            }],
            insts: vec![CanonInst {
                id: InstId(2),
                op: vm::op_local_get4_br_if as crate::common::Op,
                operands: vec![raw_local(0), crate::common::LoweredOperand::JumpTarget(2)],
                stack_before: Vec::new(),
                stack_after: Vec::new(),
                preserved_prefix_len: 0,
                fresh_result_count: 0,
                effect: EffectId(2),
            }],
            predecessors: vec![0, 2],
            successors: vec![2],
        };
        let block2 = CanonBlock {
            id: 2,
            params: Vec::new(),
            insts: vec![CanonInst {
                id: InstId(3),
                op: vm::op_br as crate::common::Op,
                operands: vec![crate::common::LoweredOperand::JumpTarget(1)],
                stack_before: Vec::new(),
                stack_after: Vec::new(),
                preserved_prefix_len: 0,
                fresh_result_count: 0,
                effect: EffectId(3),
            }],
            predecessors: vec![0, 1],
            successors: vec![1],
        };
        let func = CanonFunc {
            funcidx: crate::common::FuncIdx(0),
            functype: crate::common::FuncType(
                crate::common::ResultType(vec![]),
                crate::common::ResultType(vec![]),
            ),
            locals_size: 4,
            entry_block: 0,
            blocks: vec![block0, block1, block2],
        };
        let analysis = analysis::analyze(&func);
        let kernel = select::select(&func, &analysis);
        let versioned = versioning::apply(kernel, &func, &analysis);
        let specialized = versioned
            .blocks
            .iter()
            .filter(|block| {
                block.original_block_id == 1 && block.version.kind == BlockVersionKind::Specialized
            })
            .count();
        assert!(specialized <= 2);
    }

    #[test]
    fn optimizer_selects_call_family_registry_entries() {
        use pipeline::{
            analysis,
            ir::{CanonBlock, CanonFunc, CanonInst, EffectId, InstId},
            select,
        };

        let func = CanonFunc {
            funcidx: crate::common::FuncIdx(0),
            functype: crate::common::FuncType(
                crate::common::ResultType(vec![]),
                crate::common::ResultType(vec![]),
            ),
            locals_size: 0,
            entry_block: 0,
            blocks: vec![CanonBlock {
                id: 0,
                params: Vec::new(),
                insts: vec![CanonInst {
                    id: InstId(0),
                    op: vm::op_call_import as crate::common::Op,
                    operands: vec![crate::common::LoweredOperand::CallRecipeRef(
                        crate::common::CallRecipeRef::from_funcidx(0),
                    )],
                    stack_before: Vec::new(),
                    stack_after: Vec::new(),
                    preserved_prefix_len: 0,
                    fresh_result_count: 0,
                    effect: EffectId(0),
                }],
                predecessors: Vec::new(),
                successors: Vec::new(),
            }],
        };
        let analysis = analysis::analyze(&func);
        let kernel = select::select(&func, &analysis);
        assert_eq!(kernel.blocks[0].ops[0].family, "op_call_import");
    }

    #[test]
    fn optimizer_versioning_rewrites_direct_call_target_class() {
        use pipeline::{
            analysis,
            ir::{CanonBlock, CanonFunc, CanonInst, EffectId, InstId},
            select, versioning,
        };

        let pred = CanonBlock {
            id: 0,
            params: Vec::new(),
            insts: vec![
                CanonInst {
                    id: InstId(0),
                    op: vm::op_call_import as crate::common::Op,
                    operands: vec![crate::common::LoweredOperand::CallRecipeRef(
                        crate::common::CallRecipeRef::from_funcidx(0),
                    )],
                    stack_before: Vec::new(),
                    stack_after: Vec::new(),
                    preserved_prefix_len: 0,
                    fresh_result_count: 0,
                    effect: EffectId(0),
                },
                CanonInst {
                    id: InstId(1),
                    op: vm::op_br as crate::common::Op,
                    operands: vec![crate::common::LoweredOperand::JumpTarget(1)],
                    stack_before: Vec::new(),
                    stack_after: Vec::new(),
                    preserved_prefix_len: 0,
                    fresh_result_count: 0,
                    effect: EffectId(1),
                },
            ],
            predecessors: Vec::new(),
            successors: vec![1],
        };
        let target = CanonBlock {
            id: 1,
            params: Vec::new(),
            insts: vec![CanonInst {
                id: InstId(2),
                op: vm::op_call as crate::common::Op,
                operands: vec![crate::common::LoweredOperand::CallRecipeRef(
                    crate::common::CallRecipeRef::from_funcidx(1),
                )],
                stack_before: Vec::new(),
                stack_after: Vec::new(),
                preserved_prefix_len: 0,
                fresh_result_count: 0,
                effect: EffectId(2),
            }],
            predecessors: vec![0],
            successors: Vec::new(),
        };
        let func = CanonFunc {
            funcidx: crate::common::FuncIdx(0),
            functype: crate::common::FuncType(
                crate::common::ResultType(vec![]),
                crate::common::ResultType(vec![]),
            ),
            locals_size: 0,
            entry_block: 0,
            blocks: vec![pred, target],
        };

        let analysis = analysis::analyze(&func);
        let kernel = select::select(&func, &analysis);
        let versioned = versioning::apply(kernel, &func, &analysis);
        let specialized = versioned
            .blocks
            .iter()
            .find(|block| {
                block.original_block_id == 1
                    && matches!(block.version.kind, select::BlockVersionKind::Specialized)
            })
            .expect("specialized clone must exist");
        assert!(std::ptr::fn_addr_eq(
            specialized.ops[0].op,
            vm::op_call_import as crate::common::Op
        ));
    }

    #[test]
    fn optimizer_versioning_skips_local_nonzero_when_target_rewrites_local() {
        use crate::common::{Operand, ValType};
        use pipeline::{
            analysis,
            ir::{CanonBlock, CanonFunc, CanonInst, EffectId, InstId},
            select::{self, BlockVersionKind},
            versioning,
        };

        let pred = CanonBlock {
            id: 0,
            params: Vec::new(),
            insts: vec![
                CanonInst {
                    id: InstId(0),
                    op: vm::op_local_get4 as crate::common::Op,
                    operands: vec![raw_local(4)],
                    stack_before: Vec::new(),
                    stack_after: vec![ValType::I32],
                    preserved_prefix_len: 0,
                    fresh_result_count: 1,
                    effect: EffectId(0),
                },
                CanonInst {
                    id: InstId(1),
                    op: vm::op_br_if as crate::common::Op,
                    operands: vec![crate::common::LoweredOperand::JumpTarget(1)],
                    stack_before: vec![ValType::I32],
                    stack_after: Vec::new(),
                    preserved_prefix_len: 0,
                    fresh_result_count: 0,
                    effect: EffectId(1),
                },
            ],
            predecessors: Vec::new(),
            successors: vec![1, 3],
        };
        let target = CanonBlock {
            id: 1,
            params: Vec::new(),
            insts: vec![
                CanonInst {
                    id: InstId(2),
                    op: vm::op_i32_const as crate::common::Op,
                    operands: vec![crate::common::LoweredOperand::Raw(unsafe {
                        Operand { i32: 0 }.encoded
                    })],
                    stack_before: Vec::new(),
                    stack_after: vec![ValType::I32],
                    preserved_prefix_len: 0,
                    fresh_result_count: 1,
                    effect: EffectId(2),
                },
                CanonInst {
                    id: InstId(3),
                    op: vm::op_local_set4 as crate::common::Op,
                    operands: vec![raw_local(4)],
                    stack_before: vec![ValType::I32],
                    stack_after: Vec::new(),
                    preserved_prefix_len: 0,
                    fresh_result_count: 0,
                    effect: EffectId(3),
                },
                CanonInst {
                    id: InstId(4),
                    op: vm::op_local_get4 as crate::common::Op,
                    operands: vec![raw_local(4)],
                    stack_before: Vec::new(),
                    stack_after: vec![ValType::I32],
                    preserved_prefix_len: 0,
                    fresh_result_count: 1,
                    effect: EffectId(4),
                },
                CanonInst {
                    id: InstId(5),
                    op: vm::op_br_if as crate::common::Op,
                    operands: vec![crate::common::LoweredOperand::JumpTarget(2)],
                    stack_before: vec![ValType::I32],
                    stack_after: Vec::new(),
                    preserved_prefix_len: 0,
                    fresh_result_count: 0,
                    effect: EffectId(5),
                },
            ],
            predecessors: vec![0],
            successors: vec![2, 3],
        };
        let taken = CanonBlock {
            id: 2,
            params: Vec::new(),
            insts: vec![CanonInst {
                id: InstId(6),
                op: vm::op_i32_const as crate::common::Op,
                operands: vec![crate::common::LoweredOperand::Raw(unsafe {
                    Operand { i32: 1 }.encoded
                })],
                stack_before: Vec::new(),
                stack_after: vec![ValType::I32],
                preserved_prefix_len: 0,
                fresh_result_count: 1,
                effect: EffectId(6),
            }],
            predecessors: vec![1],
            successors: Vec::new(),
        };
        let fallthrough = CanonBlock {
            id: 3,
            params: Vec::new(),
            insts: vec![CanonInst {
                id: InstId(7),
                op: vm::op_i32_const as crate::common::Op,
                operands: vec![crate::common::LoweredOperand::Raw(unsafe {
                    Operand { i32: 2 }.encoded
                })],
                stack_before: Vec::new(),
                stack_after: vec![ValType::I32],
                preserved_prefix_len: 0,
                fresh_result_count: 1,
                effect: EffectId(7),
            }],
            predecessors: vec![0, 1],
            successors: Vec::new(),
        };
        let func = CanonFunc {
            funcidx: crate::common::FuncIdx(0),
            functype: crate::common::FuncType(
                crate::common::ResultType(vec![]),
                crate::common::ResultType(vec![]),
            ),
            locals_size: 8,
            entry_block: 0,
            blocks: vec![pred, target, taken, fallthrough],
        };

        let analysis = analysis::analyze(&func);
        let kernel = select::select(&func, &analysis);
        assert!(std::ptr::fn_addr_eq(
            kernel.blocks[1]
                .ops
                .last()
                .expect("target must have a terminator")
                .op,
            vm::op_local_get4_br_if as crate::common::Op
        ));
        let versioned = versioning::apply(kernel, &func, &analysis);
        assert!(
            versioned.blocks.iter().all(|block| {
                !(block.original_block_id == 1
                    && matches!(block.version.kind, BlockVersionKind::Specialized))
            }),
            "local fact specialization must not survive a target-side local rewrite"
        );
    }

    #[test]
    fn optimizer_analysis_tracks_live_locals() {
        use crate::common::ValType;
        use pipeline::{
            analysis,
            ir::{CanonBlock, CanonFunc, CanonInst, EffectId, InstId},
        };

        let block0 = CanonBlock {
            id: 0,
            params: Vec::new(),
            insts: vec![
                CanonInst {
                    id: InstId(0),
                    op: vm::op_local_get4 as crate::common::Op,
                    operands: vec![raw_local(4)],
                    stack_before: Vec::new(),
                    stack_after: vec![ValType::I32],
                    preserved_prefix_len: 0,
                    fresh_result_count: 1,
                    effect: EffectId(0),
                },
                CanonInst {
                    id: InstId(1),
                    op: vm::op_br_if as crate::common::Op,
                    operands: vec![crate::common::LoweredOperand::JumpTarget(1)],
                    stack_before: vec![ValType::I32],
                    stack_after: Vec::new(),
                    preserved_prefix_len: 0,
                    fresh_result_count: 0,
                    effect: EffectId(1),
                },
            ],
            predecessors: Vec::new(),
            successors: vec![1, 2],
        };
        let block1 = CanonBlock {
            id: 1,
            params: Vec::new(),
            insts: vec![
                CanonInst {
                    id: InstId(2),
                    op: vm::op_local_set4 as crate::common::Op,
                    operands: vec![raw_local(4)],
                    stack_before: vec![ValType::I32],
                    stack_after: Vec::new(),
                    preserved_prefix_len: 0,
                    fresh_result_count: 0,
                    effect: EffectId(2),
                },
                CanonInst {
                    id: InstId(3),
                    op: vm::op_br as crate::common::Op,
                    operands: vec![crate::common::LoweredOperand::JumpTarget(2)],
                    stack_before: Vec::new(),
                    stack_after: Vec::new(),
                    preserved_prefix_len: 0,
                    fresh_result_count: 0,
                    effect: EffectId(3),
                },
            ],
            predecessors: vec![0],
            successors: vec![2],
        };
        let block2 = CanonBlock {
            id: 2,
            params: Vec::new(),
            insts: vec![
                CanonInst {
                    id: InstId(4),
                    op: vm::op_local_get4 as crate::common::Op,
                    operands: vec![raw_local(4)],
                    stack_before: Vec::new(),
                    stack_after: vec![ValType::I32],
                    preserved_prefix_len: 0,
                    fresh_result_count: 1,
                    effect: EffectId(4),
                },
                CanonInst {
                    id: InstId(5),
                    op: vm::op_br as crate::common::Op,
                    operands: vec![crate::common::LoweredOperand::JumpTarget(2)],
                    stack_before: vec![ValType::I32],
                    stack_after: Vec::new(),
                    preserved_prefix_len: 0,
                    fresh_result_count: 0,
                    effect: EffectId(5),
                },
            ],
            predecessors: vec![0, 1, 2],
            successors: vec![2],
        };
        let func = CanonFunc {
            funcidx: crate::common::FuncIdx(0),
            functype: crate::common::FuncType(
                crate::common::ResultType(vec![]),
                crate::common::ResultType(vec![]),
            ),
            locals_size: 8,
            entry_block: 0,
            blocks: vec![block0, block1, block2],
        };

        let analysis = analysis::analyze(&func);
        assert_eq!(analysis.live_locals_in[2], vec![4]);
        assert_eq!(analysis.live_locals_out[0], vec![4]);
        assert!(analysis.live_locals_in[1].is_empty());
    }

    #[test]
    fn optimizer_versioning_specializes_const_base_memory_edges() {
        use crate::common::{MemArg, Operand, ValType};
        use pipeline::{
            analysis,
            ir::{
                BlockParam, CanonBlock, CanonFunc, CanonInst, EffectId, InstId, StorageClass,
                ValueId,
            },
            select::{self, BlockVersionKind},
            versioning,
        };

        let block0 = CanonBlock {
            id: 0,
            params: Vec::new(),
            insts: vec![
                CanonInst {
                    id: InstId(0),
                    op: vm::op_i32_const as crate::common::Op,
                    operands: vec![crate::common::LoweredOperand::Raw(unsafe {
                        Operand { i32: 4 }.encoded
                    })],
                    stack_before: Vec::new(),
                    stack_after: vec![ValType::I32],
                    preserved_prefix_len: 0,
                    fresh_result_count: 1,
                    effect: EffectId(0),
                },
                CanonInst {
                    id: InstId(1),
                    op: vm::op_br as crate::common::Op,
                    operands: vec![crate::common::LoweredOperand::JumpTarget(1)],
                    stack_before: vec![ValType::I32],
                    stack_after: Vec::new(),
                    preserved_prefix_len: 0,
                    fresh_result_count: 0,
                    effect: EffectId(1),
                },
            ],
            predecessors: Vec::new(),
            successors: vec![1],
        };
        let block1 = CanonBlock {
            id: 1,
            params: vec![BlockParam {
                id: ValueId(0),
                index: 0,
                ty: ValType::I32,
                storage: StorageClass::BlockParam,
            }],
            insts: vec![
                CanonInst {
                    id: InstId(2),
                    op: vm::op_i32_load as crate::common::Op,
                    operands: vec![crate::common::LoweredOperand::Raw(unsafe {
                        Operand {
                            memarg: MemArg {
                                align: 2,
                                offset: 8,
                            },
                        }
                        .encoded
                    })],
                    stack_before: vec![ValType::I32],
                    stack_after: vec![ValType::I32],
                    preserved_prefix_len: 0,
                    fresh_result_count: 1,
                    effect: EffectId(2),
                },
                CanonInst {
                    id: InstId(3),
                    op: vm::op_br as crate::common::Op,
                    operands: vec![crate::common::LoweredOperand::JumpTarget(2)],
                    stack_before: vec![ValType::I32],
                    stack_after: Vec::new(),
                    preserved_prefix_len: 0,
                    fresh_result_count: 0,
                    effect: EffectId(3),
                },
            ],
            predecessors: vec![0],
            successors: vec![2],
        };
        let block2 = CanonBlock {
            id: 2,
            params: Vec::new(),
            insts: vec![CanonInst {
                id: InstId(4),
                op: vm::op_i32_const as crate::common::Op,
                operands: vec![crate::common::LoweredOperand::Raw(unsafe {
                    Operand { i32: 0 }.encoded
                })],
                stack_before: Vec::new(),
                stack_after: vec![ValType::I32],
                preserved_prefix_len: 0,
                fresh_result_count: 1,
                effect: EffectId(4),
            }],
            predecessors: vec![1],
            successors: Vec::new(),
        };
        let func = CanonFunc {
            funcidx: crate::common::FuncIdx(0),
            functype: crate::common::FuncType(
                crate::common::ResultType(vec![]),
                crate::common::ResultType(vec![]),
            ),
            locals_size: 4,
            entry_block: 0,
            blocks: vec![block0, block1, block2],
        };

        let analysis = analysis::analyze(&func);
        let kernel = select::select(&func, &analysis);
        let versioned = versioning::apply(kernel, &func, &analysis);
        let specialized = versioned
            .blocks
            .iter()
            .find(|block| {
                block.original_block_id == 1 && block.version.kind == BlockVersionKind::Specialized
            })
            .expect("const-base edge should produce a specialized clone");

        assert!(std::ptr::fn_addr_eq(
            specialized.ops[0].op,
            vm::op_i32_load_const_base as crate::common::Op
        ));
        let crate::common::LoweredOperand::Raw(encoded) = specialized.ops[0].operands[0] else {
            panic!("expected folded memarg operand");
        };
        let memarg = unsafe { Operand { encoded }.memarg };
        assert_eq!(memarg.offset, 12);

        let entry = &versioned.blocks[0];
        assert_eq!(
            entry.ops.len(),
            1,
            "entry families={:?}",
            entry.ops.iter().map(|op| op.family).collect::<Vec<_>>()
        );
        assert!(std::ptr::fn_addr_eq(
            entry.ops[0].op,
            vm::op_br as crate::common::Op
        ));
        let crate::common::LoweredOperand::JumpTarget(target) = entry.ops[0].operands[0] else {
            panic!("expected rewritten jump target");
        };
        assert_eq!(target, specialized.label);
    }

    #[test]
    fn optimizer_versioning_specializes_i64_const_base_memory_edges() {
        use crate::common::{MemArg, Operand, ValType};
        use pipeline::{
            analysis,
            ir::{
                BlockParam, CanonBlock, CanonFunc, CanonInst, EffectId, InstId, StorageClass,
                ValueId,
            },
            select::{self, BlockVersionKind},
            versioning,
        };

        let block0 = CanonBlock {
            id: 0,
            params: Vec::new(),
            insts: vec![
                CanonInst {
                    id: InstId(0),
                    op: vm::op_i32_const as crate::common::Op,
                    operands: vec![crate::common::LoweredOperand::Raw(unsafe {
                        Operand { i32: 8 }.encoded
                    })],
                    stack_before: Vec::new(),
                    stack_after: vec![ValType::I32],
                    preserved_prefix_len: 0,
                    fresh_result_count: 1,
                    effect: EffectId(0),
                },
                CanonInst {
                    id: InstId(1),
                    op: vm::op_br as crate::common::Op,
                    operands: vec![crate::common::LoweredOperand::JumpTarget(1)],
                    stack_before: vec![ValType::I32],
                    stack_after: Vec::new(),
                    preserved_prefix_len: 0,
                    fresh_result_count: 0,
                    effect: EffectId(1),
                },
            ],
            predecessors: Vec::new(),
            successors: vec![1],
        };
        let block1 = CanonBlock {
            id: 1,
            params: vec![BlockParam {
                id: ValueId(0),
                index: 0,
                ty: ValType::I32,
                storage: StorageClass::BlockParam,
            }],
            insts: vec![
                CanonInst {
                    id: InstId(2),
                    op: vm::op_i64_load as crate::common::Op,
                    operands: vec![crate::common::LoweredOperand::Raw(unsafe {
                        Operand {
                            memarg: MemArg {
                                align: 3,
                                offset: 4,
                            },
                        }
                        .encoded
                    })],
                    stack_before: vec![ValType::I32],
                    stack_after: vec![ValType::I64],
                    preserved_prefix_len: 0,
                    fresh_result_count: 1,
                    effect: EffectId(2),
                },
                CanonInst {
                    id: InstId(3),
                    op: vm::op_br as crate::common::Op,
                    operands: vec![crate::common::LoweredOperand::JumpTarget(2)],
                    stack_before: vec![ValType::I64],
                    stack_after: Vec::new(),
                    preserved_prefix_len: 0,
                    fresh_result_count: 0,
                    effect: EffectId(3),
                },
            ],
            predecessors: vec![0],
            successors: vec![2],
        };
        let block2 = CanonBlock {
            id: 2,
            params: Vec::new(),
            insts: vec![CanonInst {
                id: InstId(4),
                op: vm::op_i64_const as crate::common::Op,
                operands: vec![crate::common::LoweredOperand::Raw(unsafe {
                    Operand { i64: 0 }.encoded
                })],
                stack_before: Vec::new(),
                stack_after: vec![ValType::I64],
                preserved_prefix_len: 0,
                fresh_result_count: 1,
                effect: EffectId(4),
            }],
            predecessors: vec![1],
            successors: Vec::new(),
        };
        let func = CanonFunc {
            funcidx: crate::common::FuncIdx(0),
            functype: crate::common::FuncType(
                crate::common::ResultType(vec![]),
                crate::common::ResultType(vec![]),
            ),
            locals_size: 4,
            entry_block: 0,
            blocks: vec![block0, block1, block2],
        };

        let analysis = analysis::analyze(&func);
        let kernel = select::select(&func, &analysis);
        let versioned = versioning::apply(kernel, &func, &analysis);
        let specialized = versioned
            .blocks
            .iter()
            .find(|block| {
                block.original_block_id == 1 && block.version.kind == BlockVersionKind::Specialized
            })
            .expect("i64 const-base edge should produce a specialized clone");

        assert!(std::ptr::fn_addr_eq(
            specialized.ops[0].op,
            vm::op_i64_load_const_base as crate::common::Op
        ));
        let crate::common::LoweredOperand::Raw(encoded) = specialized.ops[0].operands[0] else {
            panic!("expected folded memarg operand");
        };
        let memarg = unsafe { Operand { encoded }.memarg };
        assert_eq!(memarg.offset, 12);
    }

    #[test]
    fn optimizer_versioning_specializes_local_base_memory_edges() {
        use crate::common::{MemArg, Operand, ValType};
        use pipeline::{
            analysis,
            ir::{
                BlockParam, CanonBlock, CanonFunc, CanonInst, EffectId, InstId, StorageClass,
                ValueId,
            },
            select::{self, BlockVersionKind},
            versioning,
        };

        let block0 = CanonBlock {
            id: 0,
            params: Vec::new(),
            insts: vec![
                CanonInst {
                    id: InstId(0),
                    op: vm::op_local_get4 as crate::common::Op,
                    operands: vec![raw_local(4)],
                    stack_before: Vec::new(),
                    stack_after: vec![ValType::I32],
                    preserved_prefix_len: 0,
                    fresh_result_count: 1,
                    effect: EffectId(0),
                },
                CanonInst {
                    id: InstId(1),
                    op: vm::op_i32_const as crate::common::Op,
                    operands: vec![crate::common::LoweredOperand::Raw(unsafe {
                        Operand { i32: 12 }.encoded
                    })],
                    stack_before: vec![ValType::I32],
                    stack_after: vec![ValType::I32, ValType::I32],
                    preserved_prefix_len: 1,
                    fresh_result_count: 1,
                    effect: EffectId(1),
                },
                CanonInst {
                    id: InstId(2),
                    op: vm::op_i32_add as crate::common::Op,
                    operands: Vec::new(),
                    stack_before: vec![ValType::I32, ValType::I32],
                    stack_after: vec![ValType::I32],
                    preserved_prefix_len: 0,
                    fresh_result_count: 1,
                    effect: EffectId(2),
                },
                CanonInst {
                    id: InstId(3),
                    op: vm::op_br as crate::common::Op,
                    operands: vec![crate::common::LoweredOperand::JumpTarget(1)],
                    stack_before: vec![ValType::I32],
                    stack_after: Vec::new(),
                    preserved_prefix_len: 0,
                    fresh_result_count: 0,
                    effect: EffectId(3),
                },
            ],
            predecessors: Vec::new(),
            successors: vec![1],
        };
        let block1 = CanonBlock {
            id: 1,
            params: vec![BlockParam {
                id: ValueId(0),
                index: 0,
                ty: ValType::I32,
                storage: StorageClass::BlockParam,
            }],
            insts: vec![
                CanonInst {
                    id: InstId(4),
                    op: vm::op_i32_load as crate::common::Op,
                    operands: vec![crate::common::LoweredOperand::Raw(unsafe {
                        Operand {
                            memarg: MemArg {
                                align: 2,
                                offset: 8,
                            },
                        }
                        .encoded
                    })],
                    stack_before: vec![ValType::I32],
                    stack_after: vec![ValType::I32],
                    preserved_prefix_len: 0,
                    fresh_result_count: 1,
                    effect: EffectId(4),
                },
                CanonInst {
                    id: InstId(5),
                    op: vm::op_br as crate::common::Op,
                    operands: vec![crate::common::LoweredOperand::JumpTarget(2)],
                    stack_before: vec![ValType::I32],
                    stack_after: Vec::new(),
                    preserved_prefix_len: 0,
                    fresh_result_count: 0,
                    effect: EffectId(5),
                },
            ],
            predecessors: vec![0],
            successors: vec![2],
        };
        let block2 = CanonBlock {
            id: 2,
            params: Vec::new(),
            insts: vec![CanonInst {
                id: InstId(6),
                op: vm::op_i32_const as crate::common::Op,
                operands: vec![crate::common::LoweredOperand::Raw(unsafe {
                    Operand { i32: 0 }.encoded
                })],
                stack_before: Vec::new(),
                stack_after: vec![ValType::I32],
                preserved_prefix_len: 0,
                fresh_result_count: 1,
                effect: EffectId(6),
            }],
            predecessors: vec![1],
            successors: Vec::new(),
        };
        let func = CanonFunc {
            funcidx: crate::common::FuncIdx(0),
            functype: crate::common::FuncType(
                crate::common::ResultType(vec![]),
                crate::common::ResultType(vec![]),
            ),
            locals_size: 8,
            entry_block: 0,
            blocks: vec![block0, block1, block2],
        };

        let analysis = analysis::analyze(&func);
        let kernel = select::select(&func, &analysis);
        let versioned = versioning::apply(kernel, &func, &analysis);
        let specialized = versioned
            .blocks
            .iter()
            .find(|block| {
                block.original_block_id == 1 && block.version.kind == BlockVersionKind::Specialized
            })
            .expect("local-base edge should produce a specialized clone");

        assert!(std::ptr::fn_addr_eq(
            specialized.ops[0].op,
            vm::op_i32_load_local_base as crate::common::Op
        ));
        let crate::common::LoweredOperand::Raw(local_raw) = specialized.ops[0].operands[0] else {
            panic!("expected local operand");
        };
        let crate::common::LoweredOperand::Raw(delta_raw) = specialized.ops[0].operands[1] else {
            panic!("expected delta operand");
        };
        assert_eq!(unsafe { Operand { encoded: local_raw }.local_addr }, 4);
        assert_eq!(unsafe { Operand { encoded: delta_raw }.i32 }, 12);

        let entry = &versioned.blocks[0];
        assert_eq!(
            entry.ops.len(),
            1,
            "entry families={:?}",
            entry.ops.iter().map(|op| op.family).collect::<Vec<_>>()
        );
        assert!(std::ptr::fn_addr_eq(
            entry.ops[0].op,
            vm::op_br as crate::common::Op
        ));
        let crate::common::LoweredOperand::JumpTarget(target) = entry.ops[0].operands[0] else {
            panic!("expected rewritten jump target");
        };
        assert_eq!(target, specialized.label);
    }

    #[test]
    fn optimizer_versioning_specializes_local_scaled_index_memory_edges() {
        use crate::common::{MemArg, Operand, ValType};
        use pipeline::{
            analysis,
            ir::{
                BlockParam, CanonBlock, CanonFunc, CanonInst, EffectId, InstId, StorageClass,
                ValueId,
            },
            select::{self, BlockVersionKind},
            versioning,
        };

        let block0 = CanonBlock {
            id: 0,
            params: Vec::new(),
            insts: vec![
                CanonInst {
                    id: InstId(0),
                    op: vm::op_local_get4 as crate::common::Op,
                    operands: vec![raw_local(4)],
                    stack_before: Vec::new(),
                    stack_after: vec![ValType::I32],
                    preserved_prefix_len: 0,
                    fresh_result_count: 1,
                    effect: EffectId(0),
                },
                CanonInst {
                    id: InstId(1),
                    op: vm::op_local_get4 as crate::common::Op,
                    operands: vec![raw_local(8)],
                    stack_before: vec![ValType::I32],
                    stack_after: vec![ValType::I32, ValType::I32],
                    preserved_prefix_len: 1,
                    fresh_result_count: 1,
                    effect: EffectId(1),
                },
                CanonInst {
                    id: InstId(2),
                    op: vm::op_i32_const as crate::common::Op,
                    operands: vec![crate::common::LoweredOperand::Raw(unsafe {
                        Operand { i32: 2 }.encoded
                    })],
                    stack_before: vec![ValType::I32, ValType::I32],
                    stack_after: vec![ValType::I32, ValType::I32, ValType::I32],
                    preserved_prefix_len: 2,
                    fresh_result_count: 1,
                    effect: EffectId(2),
                },
                CanonInst {
                    id: InstId(3),
                    op: vm::op_i32_shl as crate::common::Op,
                    operands: Vec::new(),
                    stack_before: vec![ValType::I32, ValType::I32, ValType::I32],
                    stack_after: vec![ValType::I32, ValType::I32],
                    preserved_prefix_len: 1,
                    fresh_result_count: 1,
                    effect: EffectId(3),
                },
                CanonInst {
                    id: InstId(4),
                    op: vm::op_i32_add as crate::common::Op,
                    operands: Vec::new(),
                    stack_before: vec![ValType::I32, ValType::I32],
                    stack_after: vec![ValType::I32],
                    preserved_prefix_len: 0,
                    fresh_result_count: 1,
                    effect: EffectId(4),
                },
                CanonInst {
                    id: InstId(5),
                    op: vm::op_i32_const as crate::common::Op,
                    operands: vec![crate::common::LoweredOperand::Raw(unsafe {
                        Operand { i32: 16 }.encoded
                    })],
                    stack_before: vec![ValType::I32],
                    stack_after: vec![ValType::I32, ValType::I32],
                    preserved_prefix_len: 1,
                    fresh_result_count: 1,
                    effect: EffectId(5),
                },
                CanonInst {
                    id: InstId(6),
                    op: vm::op_i32_add as crate::common::Op,
                    operands: Vec::new(),
                    stack_before: vec![ValType::I32, ValType::I32],
                    stack_after: vec![ValType::I32],
                    preserved_prefix_len: 0,
                    fresh_result_count: 1,
                    effect: EffectId(6),
                },
                CanonInst {
                    id: InstId(7),
                    op: vm::op_br as crate::common::Op,
                    operands: vec![crate::common::LoweredOperand::JumpTarget(1)],
                    stack_before: vec![ValType::I32],
                    stack_after: Vec::new(),
                    preserved_prefix_len: 0,
                    fresh_result_count: 0,
                    effect: EffectId(7),
                },
            ],
            predecessors: Vec::new(),
            successors: vec![1],
        };
        let block1 = CanonBlock {
            id: 1,
            params: vec![BlockParam {
                id: ValueId(0),
                index: 0,
                ty: ValType::I32,
                storage: StorageClass::BlockParam,
            }],
            insts: vec![
                CanonInst {
                    id: InstId(8),
                    op: vm::op_i32_load as crate::common::Op,
                    operands: vec![crate::common::LoweredOperand::Raw(unsafe {
                        Operand {
                            memarg: MemArg {
                                align: 2,
                                offset: 4,
                            },
                        }
                        .encoded
                    })],
                    stack_before: vec![ValType::I32],
                    stack_after: vec![ValType::I32],
                    preserved_prefix_len: 0,
                    fresh_result_count: 1,
                    effect: EffectId(8),
                },
                CanonInst {
                    id: InstId(9),
                    op: vm::op_br as crate::common::Op,
                    operands: vec![crate::common::LoweredOperand::JumpTarget(2)],
                    stack_before: vec![ValType::I32],
                    stack_after: Vec::new(),
                    preserved_prefix_len: 0,
                    fresh_result_count: 0,
                    effect: EffectId(9),
                },
            ],
            predecessors: vec![0],
            successors: vec![2],
        };
        let block2 = CanonBlock {
            id: 2,
            params: Vec::new(),
            insts: vec![CanonInst {
                id: InstId(10),
                op: vm::op_i32_const as crate::common::Op,
                operands: vec![crate::common::LoweredOperand::Raw(unsafe {
                    Operand { i32: 0 }.encoded
                })],
                stack_before: Vec::new(),
                stack_after: vec![ValType::I32],
                preserved_prefix_len: 0,
                fresh_result_count: 1,
                effect: EffectId(10),
            }],
            predecessors: vec![1],
            successors: Vec::new(),
        };
        let func = CanonFunc {
            funcidx: crate::common::FuncIdx(0),
            functype: crate::common::FuncType(
                crate::common::ResultType(vec![]),
                crate::common::ResultType(vec![]),
            ),
            locals_size: 12,
            entry_block: 0,
            blocks: vec![block0, block1, block2],
        };

        let analysis = analysis::analyze(&func);
        let kernel = select::select(&func, &analysis);
        let versioned = versioning::apply(kernel, &func, &analysis);
        let specialized = versioned
            .blocks
            .iter()
            .find(|block| {
                block.original_block_id == 1 && block.version.kind == BlockVersionKind::Specialized
            })
            .expect("local-scaled-index edge should produce a specialized clone");

        assert!(std::ptr::fn_addr_eq(
            specialized.ops[0].op,
            vm::op_i32_load_local_scaled_index as crate::common::Op
        ));
        let crate::common::LoweredOperand::Raw(base_raw) = specialized.ops[0].operands[0] else {
            panic!("expected base operand");
        };
        let crate::common::LoweredOperand::Raw(index_raw) = specialized.ops[0].operands[1] else {
            panic!("expected index operand");
        };
        let crate::common::LoweredOperand::Raw(scale_raw) = specialized.ops[0].operands[2] else {
            panic!("expected scale operand");
        };
        let crate::common::LoweredOperand::Raw(delta_raw) = specialized.ops[0].operands[3] else {
            panic!("expected delta operand");
        };
        assert_eq!(unsafe { Operand { encoded: base_raw }.local_addr }, 4);
        assert_eq!(unsafe { Operand { encoded: index_raw }.local_addr }, 8);
        assert_eq!(unsafe { Operand { encoded: scale_raw }.u32 }, 2);
        assert_eq!(unsafe { Operand { encoded: delta_raw }.i32 }, 16);

        let entry = &versioned.blocks[0];
        assert_eq!(
            entry.ops.len(),
            1,
            "entry families={:?}",
            entry.ops.iter().map(|op| op.family).collect::<Vec<_>>()
        );
        assert!(std::ptr::fn_addr_eq(
            entry.ops[0].op,
            vm::op_br as crate::common::Op
        ));
        let crate::common::LoweredOperand::JumpTarget(target) = entry.ops[0].operands[0] else {
            panic!("expected rewritten jump target");
        };
        assert_eq!(target, specialized.label);
    }

    #[test]
    fn optimizer_handles_small_call_loop_without_hanging() {
        let func = function_at(
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
        let materialized = func.lowered.materialize();
        assert!(!func.lowered.code.is_empty());
        assert_control_targets_align(&materialized.instrs, &materialized.op_lens);
    }
}
