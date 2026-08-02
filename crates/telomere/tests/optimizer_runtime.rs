mod common;

use common::{instantiate_wat, run_wast};
use telomere::{run_module_function, Registry, ResultValue, Store, VMResult, WasmValue};

#[tokio::test]
async fn optimizer_small_call_loop_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
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
        &store,
        &registry,
    )
    .await;

    let result = run_module_function(
        &instance,
        &store,
        "run",
        &ResultValue::new(vec![WasmValue::I32(64)]),
    )
    .await;

    match result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(64)]));
        }
        other => panic!("call loop must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_direct_call_with_mixed_local_const_args_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func $mix (param i32 i32 i32) (result i32)
            local.get 0
            local.get 1
            i32.add
            local.get 2
            i32.add)
          (func (export "run") (param i32) (result i32)
            local.get 0
            i32.const 4
            local.get 0
            call $mix))
        "#,
        &store,
        &registry,
    )
    .await;

    for (input, expected) in [(0, 4), (1, 6), (7, 18)] {
        let result = run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![WasmValue::I32(input)]),
        )
        .await;

        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => panic!("mixed direct call({input}) must succeed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn optimizer_direct_call_cached_u16_guard_rejects_wrong_return_mask() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data (i32.const 0) "\c0\00")
          (func $almost_cached_u16_low7_guard
            (param $data i32)
            (param $ctx i32)
            (result i32)
            (local $cached i32)
            local.get $data
            i32.load16_u
            local.tee $cached
            i32.const 128
            i32.and
            if
              local.get $cached
              i32.const 63
              i32.and
              return
            end
            i32.const 7)
          (func (export "run") (result i32)
            i32.const 0
            i32.const 0
            call $almost_cached_u16_low7_guard))
        "#,
        &store,
        &registry,
    )
    .await;

    let result = run_module_function(&instance, &store, "run", &ResultValue::new(vec![])).await;

    match result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(0)]));
        }
        other => panic!("almost cached-u16 guard call must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_small_memory_loop_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
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
        &store,
        &registry,
    )
    .await;

    let result = run_module_function(
        &instance,
        &store,
        "run",
        &ResultValue::new(vec![WasmValue::I32(64)]),
    )
    .await;

    match result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(64)]));
        }
        other => panic!("memory loop must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_memory_address_select_tree_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data (i32.const 0) "\11\00\00\00\22\00\00\00")
          (global $base (mut i32) (i32.const 0))
          (func (export "run") (param i32) (result i32)
            global.get $base
            i32.const 4
            local.get 0
            select
            i32.load))
        "#,
        &store,
        &registry,
    )
    .await;

    for (flag, expected) in [(0, 34), (1, 17)] {
        let result = run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![WasmValue::I32(flag)]),
        )
        .await;

        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => panic!("memory address select({flag}) must succeed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn optimizer_select_fast_path_preserves_global_set_i32_in_wast_flow() {
    run_wast(
        r#"
        (module
          (global $g (mut i32) (i32.const 10))
          (func (export "run") (param i32) (result i32)
            (global.set $g (select (i32.const 1) (i32.const 2) (local.get 0)))
            (global.get $g)))

        (assert_return (invoke "run" (i32.const 0)) (i32.const 2))
        (assert_return (invoke "run" (i32.const 1)) (i32.const 1))
        "#,
    )
    .await;
}

#[tokio::test]
async fn optimizer_stack_const_binop_and_select_tee_remain_correct() {
    run_wast(
        r#"
        (module
          (func (export "binop") (param i32 i32) (result i32)
            local.get 0
            local.get 1
            i32.add
            i32.const 7
            i32.xor)

          (func (export "tee") (param i32 i32) (result i32)
            (local i32)
            local.get 0
            local.get 1
            i32.add
            i32.const 1
            i32.shr_u
            local.tee 2)

          (func (export "cmp") (param i32 i32) (result i32)
            local.get 0
            local.get 1
            i32.add
            i32.const 10
            i32.lt_u)

          (func (export "select_tee") (param i32 i32 i32) (result i32)
            (local i32)
            local.get 0
            local.get 1
            local.get 2
            select
            local.tee 3
            local.get 3
            i32.add)

          (func (export "local_pair") (param i32 i32) (result i32)
            local.get 0
            local.get 1
            drop)

          (func (export "local_triple") (param i32 i32 i32) (result i32)
            local.get 0
            local.get 1
            local.get 2
            drop
            i32.add)

          (func (export "local_run4") (param i32 i32 i32 i32) (result i32)
            local.get 0
            local.get 1
            local.get 2
            local.get 3
            drop
            drop
            i32.add)

          (memory 1)
          (global $load_addr (mut i32) (i32.const 32))
          (data (i32.const 0) "\01\00\02\00\03\00")
          (data (i32.const 16) "\04\00\05\00\06\00")
          (data (i32.const 32) "\05\00\00\00\00\00\00\00")
          (data (i32.const 48) "\aa\00\bb\00\cc\00")
          (data (i32.const 64) "\bb\00\dd\00")
          (func (export "load_then_store") (param i32 i32) (result i32)
            global.get $load_addr
            i32.load
            local.get 1
            local.get 0
            i32.store)
          (func (export "load_written") (param i32) (result i32)
            local.get 0
            i32.load)
          (func (export "narrow_cmp") (param i32 i32) (result i32)
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
            local.get 2
            local.get 3
            i32.add)
          (func (export "dot") (param i32 i32 i32) (result i32)
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

        (assert_return (invoke "binop" (i32.const 2) (i32.const 3)) (i32.const 2))
        (assert_return (invoke "tee" (i32.const 10) (i32.const 4)) (i32.const 7))
        (assert_return (invoke "cmp" (i32.const 2) (i32.const 3)) (i32.const 1))
        (assert_return (invoke "cmp" (i32.const 9) (i32.const 2)) (i32.const 0))
        (assert_return (invoke "select_tee" (i32.const 11) (i32.const 20) (i32.const 0)) (i32.const 40))
        (assert_return (invoke "select_tee" (i32.const 11) (i32.const 20) (i32.const 1)) (i32.const 22))
        (assert_return (invoke "local_pair" (i32.const 12) (i32.const 34)) (i32.const 12))
        (assert_return (invoke "local_triple" (i32.const 12) (i32.const 34) (i32.const 56)) (i32.const 46))
        (assert_return (invoke "local_run4" (i32.const 12) (i32.const 34) (i32.const 56) (i32.const 78)) (i32.const 46))
        (assert_return (invoke "load_then_store" (i32.const 99) (i32.const 36)) (i32.const 5))
        (assert_return (invoke "load_written" (i32.const 36)) (i32.const 99))
        (assert_return (invoke "narrow_cmp" (i32.const 48) (i32.const 64)) (i32.const 374))
        (assert_return (invoke "narrow_cmp" (i32.const 48) (i32.const 66)) (i32.const 7))
        (assert_return (invoke "dot" (i32.const 0) (i32.const 16) (i32.const 3)) (i32.const 32))
        "#,
    )
    .await;
}

#[tokio::test]
async fn optimizer_i32_select_bit_step4_remains_correct() {
    run_wast(
        r#"
        (module
          (func (export "xor_step") (param $data i32) (param $crc i32) (result i32)
            (local $tmp i32)
            local.get $crc
            i32.const 1
            i32.shr_u
            local.tee $tmp
            i32.const -24575
            i32.xor
            local.get $tmp
            local.get $data
            local.get $crc
            i32.xor
            i32.const 1
            i32.and
            select)

          (func (export "eq_step") (param $data i32) (param $crc i32) (result i32)
            (local $tmp i32) (local $dst i32)
            local.get $crc
            i32.const 1
            i32.shr_u
            i32.const 32767
            i32.and
            local.tee $tmp
            local.get $tmp
            i32.const 40961
            i32.xor
            local.get $crc
            i32.const 1
            i32.and
            local.get $data
            i32.const 15
            i32.shr_u
            i32.eq
            select
            local.tee $dst)

          (func (export "run2") (param $data i32) (param $crc i32) (result i32)
            (local $tmp i32)
            local.get $crc
            i32.const 1
            i32.shr_u
            local.tee $tmp
            i32.const -24575
            i32.xor
            local.get $tmp
            local.get $data
            local.get $crc
            i32.xor
            i32.const 1
            i32.and
            select
            local.tee $crc
            i32.const 1
            i32.shr_u
            i32.const 32767
            i32.and
            local.tee $tmp
            i32.const -24575
            i32.xor
            local.get $tmp
            local.get $data
            i32.const 1
            i32.shr_u
            local.get $crc
            i32.xor
            i32.const 1
            i32.and
            select))

        (assert_return (invoke "xor_step" (i32.const 1) (i32.const 0)) (i32.const -24575))
        (assert_return (invoke "xor_step" (i32.const 0) (i32.const 2)) (i32.const 1))
        (assert_return (invoke "eq_step" (i32.const 32768) (i32.const 1)) (i32.const 0))
        (assert_return (invoke "eq_step" (i32.const 0) (i32.const 1)) (i32.const 40961))
        (assert_return (invoke "run2" (i32.const 1) (i32.const 0)) (i32.const -4095))
        "#,
    )
    .await;
}

#[tokio::test]
async fn optimizer_load8_set4_local_get_neighbors_remain_correct() {
    run_wast(
        r#"
        (module
          (memory 1)
          (data (i32.const 0) "\7f")
          (func (export "before") (param $ptr i32) (result i32 i32)
            (local $dst i32)
            i32.const 9
            local.set $dst
            local.get $dst
            local.get $ptr
            i32.load8_u
            local.set $dst
            local.get $dst)
          (func (export "after") (param $ptr i32) (result i32)
            (local $dst i32)
            local.get $ptr
            i32.load8_u
            local.set $dst
            local.get $dst))

        (assert_return (invoke "before" (i32.const 0)) (i32.const 9) (i32.const 127))
        (assert_return (invoke "after" (i32.const 0)) (i32.const 127))
        "#,
    )
    .await;
}

#[tokio::test]
async fn optimizer_select_fast_path_preserves_global_set_i64_in_wast_flow() {
    run_wast(
        r#"
        (module
          (global $g (mut i64) (i64.const 10))
          (func (export "run") (param i32) (result i64)
            (global.set $g (select (i64.const 1) (i64.const 2) (local.get 0)))
            (global.get $g)))

        (assert_return (invoke "run" (i32.const 0)) (i64.const 2))
        (assert_return (invoke "run" (i32.const 1)) (i64.const 1))
        "#,
    )
    .await;
}

#[tokio::test]
async fn optimizer_select_wast_prefix_through_global_set_remains_correct() {
    let prefix = include_str!("wasm-testsuite/select.wast")
        .lines()
        .take(302)
        .collect::<Vec<_>>()
        .join("\n");
    run_wast(&prefix).await;
}

#[test]
fn optimizer_select_wast_prefix_through_global_set_remains_correct_on_current_thread_runtime() {
    let prefix = include_str!("wasm-testsuite/select.wast")
        .lines()
        .take(302)
        .collect::<Vec<_>>()
        .join("\n");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime must build");
    runtime.block_on(async {
        run_wast(&prefix).await;
    });
}

#[test]
fn optimizer_select_full_wast_remains_correct_on_current_thread_runtime() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime must build");
    runtime.block_on(async {
        run_wast(include_str!("wasm-testsuite/select.wast")).await;
    });
}

#[test]
fn optimizer_select_memory_operand_sequence_remains_correct_on_current_thread_runtime() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime must build");
    runtime.block_on(async {
        run_wast(
            r#"
            (module
              (memory 1)
              (func (export "as-store-first") (param i32)
                (select (i32.const 0) (i32.const 4) (local.get 0)) (i32.const 1) (i32.store)
              )
              (func (export "as-store-last") (param i32)
                (i32.const 8) (select (i32.const 1) (i32.const 2) (local.get 0)) (i32.store)
              )
              (func (export "as-load-operand") (param i32) (result i32)
                (i32.load (select (i32.const 0) (i32.const 4) (local.get 0)))
              )
              (func (export "load0") (result i32)
                (i32.load (i32.const 0))
              )
              (func (export "load4") (result i32)
                (i32.load (i32.const 4))
              ))

            (assert_return (invoke "as-store-first" (i32.const 0)))
            (assert_return (invoke "load0") (i32.const 0))
            (assert_return (invoke "load4") (i32.const 1))
            (assert_return (invoke "as-store-first" (i32.const 1)))
            (assert_return (invoke "load0") (i32.const 1))
            (assert_return (invoke "load4") (i32.const 1))
            (assert_return (invoke "as-store-last" (i32.const 0)))
            (assert_return (invoke "as-store-last" (i32.const 1)))
            (assert_return (invoke "as-load-operand" (i32.const 0)) (i32.const 1))
            (assert_return (invoke "as-load-operand" (i32.const 1)) (i32.const 1))
            "#,
        )
        .await;
    });
}

#[cfg(feature = "simd")]
#[tokio::test]
async fn optimizer_select_fast_path_preserves_global_set_v128_in_wast_flow() {
    run_wast(
        r#"
        (module
          (global $g (mut v128) (v128.const i32x4 0 0 0 0))
          (func (export "run") (param i32) (result v128)
            (global.set $g
              (select
                (v128.const i32x4 1 2 3 4)
                (v128.const i32x4 5 6 7 8)
                (local.get 0)))
            (global.get $g)))

        (assert_return (invoke "run" (i32.const 0)) (v128.const i32x4 5 6 7 8))
        (assert_return (invoke "run" (i32.const 1)) (v128.const i32x4 1 2 3 4))
        "#,
    )
    .await;
}

#[tokio::test]
async fn optimizer_select_fast_path_preserves_store_then_load_address_flow() {
    run_wast(
        r#"
        (module
          (memory 1)
          (func (export "probe-addr") (param i32) (result i32)
            (select (i32.const 0) (i32.const 4) (local.get 0)))
          (func (export "probe-const") (param i32) (result i32)
            (select (i32.const 0) (i32.const 4) (local.get 0))
            (drop)
            (i32.const 1))
          (func (export "probe-after-const-pop") (param i32) (result i32) (local i32)
            (select (i32.const 0) (i32.const 4) (local.get 0))
            (i32.const 1)
            (local.set 1))
          (func (export "as-store-first") (param i32)
            (select (i32.const 0) (i32.const 4) (local.get 0))
            (i32.const 1)
            (i32.store))
          (func (export "as-store-last") (param i32)
            (i32.const 8)
            (select (i32.const 1) (i32.const 2) (local.get 0))
            (i32.store))
          (func (export "as-load-operand") (param i32) (result i32)
            (i32.load (select (i32.const 0) (i32.const 4) (local.get 0)))))

        (assert_return (invoke "probe-addr" (i32.const 0)) (i32.const 4))
        (assert_return (invoke "probe-addr" (i32.const 1)) (i32.const 0))
        (assert_return (invoke "probe-const" (i32.const 0)) (i32.const 1))
        (assert_return (invoke "probe-const" (i32.const 1)) (i32.const 1))
        (assert_return (invoke "probe-after-const-pop" (i32.const 0)) (i32.const 4))
        (assert_return (invoke "probe-after-const-pop" (i32.const 1)) (i32.const 0))
        (assert_return (invoke "as-store-first" (i32.const 0)))
        (assert_return (invoke "as-store-first" (i32.const 1)))
        (assert_return (invoke "as-store-last" (i32.const 0)))
        (assert_return (invoke "as-store-last" (i32.const 1)))
        (assert_return (invoke "as-load-operand" (i32.const 0)) (i32.const 1))
        (assert_return (invoke "as-load-operand" (i32.const 1)) (i32.const 1))
        "#,
    )
    .await;
}

#[tokio::test]
async fn optimizer_select_fast_path_stores_to_selected_address() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (func (export "as-store-first") (param i32)
            (select (i32.const 0) (i32.const 4) (local.get 0))
            (i32.const 1)
            (i32.store))
          (func (export "load0") (result i32)
            (i32.load (i32.const 0)))
          (func (export "load4") (result i32)
            (i32.load (i32.const 4))))
        "#,
        &store,
        &registry,
    )
    .await;

    let store_result = run_module_function(
        &instance,
        &store,
        "as-store-first",
        &ResultValue::new(vec![WasmValue::I32(0)]),
    )
    .await;
    assert!(matches!(store_result, VMResult::Success(values) if values.is_empty()));

    let load0 = run_module_function(&instance, &store, "load0", &ResultValue::new(vec![])).await;
    let load4 = run_module_function(&instance, &store, "load4", &ResultValue::new(vec![])).await;

    match load0 {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(0)]));
        }
        other => panic!("load0 after store-first(0) must succeed, got {other:?}"),
    }
    match load4 {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(1)]));
        }
        other => panic!("load4 after store-first(0) must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_memory_address_const_and_eqz_roots_remain_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data (i32.const 0) "\11\22\00\00\2a\00\00\00")
          (func (export "run_const") (param i32) (result i32)
            i32.const 4
            local.get 0
            drop
            i32.load)
          (func (export "run_eqz") (param i32) (result i32)
            local.get 0
            i32.eqz
            i32.const 9
            drop
            i32.load8_u))
        "#,
        &store,
        &registry,
    )
    .await;

    let const_result = run_module_function(
        &instance,
        &store,
        "run_const",
        &ResultValue::new(vec![WasmValue::I32(7)]),
    )
    .await;
    match const_result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(42)]));
        }
        other => panic!("const-root memory address path must succeed, got {other:?}"),
    }

    for (flag, expected) in [(0, 34), (5, 17)] {
        let eqz_result = run_module_function(
            &instance,
            &store,
            "run_eqz",
            &ResultValue::new(vec![WasmValue::I32(flag)]),
        )
        .await;
        match eqz_result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => panic!("eqz-root memory address path({flag}) must succeed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn optimizer_memory_address_call_root_with_store_value_suffix_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (func $addr (result i32)
            i32.const 0)
          (func (export "run") (param i32 i32) (result i32)
            call $addr
            local.get 0
            local.get 1
            i32.add
            i32.store
            call $addr
            i32.load))
        "#,
        &store,
        &registry,
    )
    .await;

    for ((lhs, rhs), expected) in [((0, 0), 0), ((7, 3), 10), ((11, 5), 16)] {
        let result = run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![WasmValue::I32(lhs), WasmValue::I32(rhs)]),
        )
        .await;

        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => panic!(
                "memory address call-root store suffix({lhs}, {rhs}) must succeed, got {other:?}"
            ),
        }
    }
}

#[tokio::test]
async fn optimizer_local_base_store_with_add_value_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (func (export "run") (param $base i32) (param $lhs i32) (param $rhs i32) (result i32)
            local.get $base
            i32.const 4
            i32.add
            local.get $lhs
            local.get $rhs
            i32.add
            i32.store
            local.get $base
            i32.const 4
            i32.add
            i32.load))
        "#,
        &store,
        &registry,
    )
    .await;

    for ((base, lhs, rhs), expected) in [((0, 7, 3), 10), ((8, 11, 5), 16)] {
        let result = run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![
                WasmValue::I32(base),
                WasmValue::I32(lhs),
                WasmValue::I32(rhs),
            ]),
        )
        .await;

        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => panic!(
                "local-base store add-value({base}, {lhs}, {rhs}) must succeed, got {other:?}"
            ),
        }
    }
}

#[tokio::test]
async fn optimizer_local_base_store_with_local_value_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (func (export "run") (param $base i32) (param $value i32) (result i32)
            local.get $base
            i32.const 4
            i32.add
            local.get $value
            i32.store
            local.get $base
            i32.const 4
            i32.add
            i32.load))
        "#,
        &store,
        &registry,
    )
    .await;

    for ((base, value), expected) in [((0, 11), 11), ((8, 22), 22)] {
        let result = run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![WasmValue::I32(base), WasmValue::I32(value)]),
        )
        .await;

        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => {
                panic!("local-base store local-value({base}, {value}) must succeed, got {other:?}")
            }
        }
    }
}

#[tokio::test]
async fn optimizer_local_get4_local_base_load_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data (i32.const 8) "\2a\00\00\00\7f")
          (func (export "load32") (param $preserved i32) (param $addr i32) (result i32 i32)
            local.get $preserved
            local.get $addr
            i32.load)
          (func (export "load8") (param $preserved i32) (param $addr i32) (result i32 i32)
            local.get $preserved
            local.get $addr
            i32.load8_u))
        "#,
        &store,
        &registry,
    )
    .await;

    let result = run_module_function(
        &instance,
        &store,
        "load32",
        &ResultValue::new(vec![WasmValue::I32(17), WasmValue::I32(8)]),
    )
    .await;
    match result {
        VMResult::Success(values) => {
            assert_eq!(
                values,
                ResultValue::new(vec![WasmValue::I32(17), WasmValue::I32(42)])
            );
        }
        other => panic!("local-get local-base load32 must succeed, got {other:?}"),
    }

    let result = run_module_function(
        &instance,
        &store,
        "load8",
        &ResultValue::new(vec![WasmValue::I32(23), WasmValue::I32(12)]),
    )
    .await;
    match result {
        VMResult::Success(values) => {
            assert_eq!(
                values,
                ResultValue::new(vec![WasmValue::I32(23), WasmValue::I32(127)])
            );
        }
        other => panic!("local-get local-base load8 must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_local_base_load_add_set_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data (i32.const 8) "\25\00\00\00")
          (func (export "run") (param $base i32) (param $rhs i32) (result i32)
            (local $dst i32)
            local.get $rhs
            local.get $base
            i32.load
            i32.add
            local.set $dst
            local.get $dst))
        "#,
        &store,
        &registry,
    )
    .await;

    let result = run_module_function(
        &instance,
        &store,
        "run",
        &ResultValue::new(vec![WasmValue::I32(8), WasmValue::I32(5)]),
    )
    .await;

    match result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(42)]));
        }
        other => panic!("local-base load add-set must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_local_base_load_set4_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data (i32.const 8) "\ff\34\12")
          (func (export "load8_s") (param $base i32) (result i32)
            (local $dst i32)
            local.get $base
            i32.load8_s
            local.set $dst
            local.get $dst)
          (func (export "load16_u") (param $base i32) (result i32)
            (local $dst i32)
            local.get $base
            i32.load16_u offset=1
            local.tee $dst))
        "#,
        &store,
        &registry,
    )
    .await;

    let load8 = run_module_function(
        &instance,
        &store,
        "load8_s",
        &ResultValue::new(vec![WasmValue::I32(8)]),
    )
    .await;
    match load8 {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(-1)]));
        }
        other => panic!("local-base load8_s set must succeed, got {other:?}"),
    }

    let load16 = run_module_function(
        &instance,
        &store,
        "load16_u",
        &ResultValue::new(vec![WasmValue::I32(8)]),
    )
    .await;
    match load16 {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(0x1234)]));
        }
        other => panic!("local-base load16_u tee must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_local_base_load_tee_branch_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data (i32.const 8) "\2a\00\00\00\00\00\00\00")
          (func (export "br_if") (param $base i32) (result i32)
            (local $dst i32)
            block
              local.get $base
              i32.load
              local.tee $dst
              br_if 0
              i32.const 7
              return
            end
            local.get $dst)
          (func (export "eqz_br_if") (param $base i32) (result i32)
            (local $dst i32)
            block
              local.get $base
              i32.load
              local.tee $dst
              i32.eqz
              br_if 0
              i32.const 7
              return
            end
            local.get $dst)
          (func (export "br_if8_u") (param $base i32) (result i32)
            (local $dst i32)
            block
              local.get $base
              i32.load8_u
              local.tee $dst
              br_if 0
              i32.const 7
              return
            end
            local.get $dst)
          (func (export "eqz_br_if16_s") (param $base i32) (result i32)
            (local $dst i32)
            block
              local.get $base
              i32.load16_s
              local.tee $dst
              i32.eqz
              br_if 0
              i32.const 7
              return
            end
            local.get $dst))
        "#,
        &store,
        &registry,
    )
    .await;

    for (name, base, expected) in [
        ("br_if", 8, 42),
        ("br_if", 12, 7),
        ("eqz_br_if", 8, 7),
        ("eqz_br_if", 12, 0),
        ("br_if8_u", 8, 42),
        ("br_if8_u", 12, 7),
        ("eqz_br_if16_s", 8, 7),
        ("eqz_br_if16_s", 12, 0),
    ] {
        let result = run_module_function(
            &instance,
            &store,
            name,
            &ResultValue::new(vec![WasmValue::I32(base)]),
        )
        .await;
        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => panic!("local-base load tee branch `{name}` must succeed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn optimizer_dot4_local_base_loop_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data (i32.const 8) "\01\00\02\00\03\00\04\00")
          (data (i32.const 16) "\05\00\06\00\07\00\08\00")
          (func (export "dot") (param $a_base i32) (param $b_base i32) (param $limit i32) (param $acc i32)
            (result i32 i32 i32)
            (local $idx i32)
            (local $counter i32)
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
            local.get $acc
            local.get $idx
            local.get $counter))
        "#,
        &store,
        &registry,
    )
    .await;

    let result = run_module_function(
        &instance,
        &store,
        "dot",
        &ResultValue::new(vec![
            WasmValue::I32(8),
            WasmValue::I32(16),
            WasmValue::I32(4),
            WasmValue::I32(5),
        ]),
    )
    .await;
    match result {
        VMResult::Success(values) => {
            assert_eq!(
                values,
                ResultValue::new(vec![
                    WasmValue::I32(75),
                    WasmValue::I32(8),
                    WasmValue::I32(4)
                ])
            );
        }
        other => panic!("dot4 local-base loop must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_i32_load16_s_mul_add_local_base_delta_loop_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data (i32.const 8) "\02\00\00\00\03\00\00\00\04\00")
          (data (i32.const 32) "\05\00\06\00\07\00")
          (func (export "dot_stride")
            (param $a_start i32)
            (param $b_start i32)
            (param $count i32)
            (param $a_stride i32)
            (param $acc i32)
            (result i32 i32 i32 i32)
            (local $a i32)
            (local $b i32)
            (local $counter i32)
            local.get $a_start
            local.set $a
            local.get $b_start
            local.set $b
            local.get $count
            local.set $counter
            loop $again
              local.get $a
              i32.load16_s
              local.get $b
              i32.load16_s
              i32.mul
              local.get $acc
              i32.add
              local.set $acc
              local.get $b
              i32.const 2
              i32.add
              local.set $b
              local.get $a
              local.get $a_stride
              i32.add
              local.set $a
              local.get $counter
              i32.const -1
              i32.add
              local.tee $counter
              br_if $again
            end
            local.get $acc
            local.get $a
            local.get $b
            local.get $counter))
        "#,
        &store,
        &registry,
    )
    .await;

    let result = run_module_function(
        &instance,
        &store,
        "dot_stride",
        &ResultValue::new(vec![
            WasmValue::I32(8),
            WasmValue::I32(32),
            WasmValue::I32(3),
            WasmValue::I32(4),
            WasmValue::I32(1),
        ]),
    )
    .await;
    match result {
        VMResult::Success(values) => {
            assert_eq!(
                values,
                ResultValue::new(vec![
                    WasmValue::I32(57),
                    WasmValue::I32(20),
                    WasmValue::I32(38),
                    WasmValue::I32(0)
                ])
            );
        }
        other => panic!("dot16 local-base delta loop must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_i32_load16_u_bitmix_acc_local_base_delta_loop_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data (i32.const 8) "\0a\00\0c\00\0e\00")
          (data (i32.const 32) "\09\00\0b\00\0d\00")
          (func (export "bitmix")
            (param $a_start i32)
            (param $b_start i32)
            (param $count i32)
            (param $a_stride i32)
            (param $acc i32)
            (result i32 i32 i32 i32)
            (local $a i32)
            (local $b i32)
            (local $counter i32)
            local.get $a_start
            local.set $a
            local.get $b_start
            local.set $b
            local.get $count
            local.set $counter
            loop $again
              local.get $acc
              local.get $a
              i32.load16_u
              local.get $b
              i32.load16_u
              i32.mul
              local.tee $acc
              i32.const 2
              i32.shr_u
              i32.const 15
              i32.and
              local.get $acc
              i32.const 5
              i32.shr_u
              i32.const 127
              i32.and
              i32.mul
              i32.add
              local.set $acc
              local.get $b
              i32.const 2
              i32.add
              local.set $b
              local.get $a
              local.get $a_stride
              i32.add
              local.set $a
              local.get $counter
              i32.const -1
              i32.add
              local.tee $counter
              br_if $again
            end
            local.get $acc
            local.get $a
            local.get $b
            local.get $counter))
        "#,
        &store,
        &registry,
    )
    .await;

    let result = run_module_function(
        &instance,
        &store,
        "bitmix",
        &ResultValue::new(vec![
            WasmValue::I32(8),
            WasmValue::I32(32),
            WasmValue::I32(3),
            WasmValue::I32(2),
            WasmValue::I32(1),
        ]),
    )
    .await;
    match result {
        VMResult::Success(values) => {
            assert_eq!(
                values,
                ResultValue::new(vec![
                    WasmValue::I32(82),
                    WasmValue::I32(14),
                    WasmValue::I32(38),
                    WasmValue::I32(0)
                ])
            );
        }
        other => panic!("load16_u bitmix loop must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_i32_load16_u_update_store16_local_base_loop_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data (i32.const 8) "\01\00\02\00\ff\ff")
          (data (i32.const 32) "\0a\00\14\00\1e\00\00\00")
          (func (export "add_loop") (param $ptr i32) (param $scalar i32) (param $count i32)
            (result i32 i32 i32 i32 i32)
            (local $p i32)
            (local $c i32)
            local.get $ptr
            local.set $p
            local.get $count
            local.set $c
            loop $again
              local.get $p
              local.get $p
              i32.load16_u
              local.get $scalar
              i32.add
              i32.store16
              local.get $p
              i32.const 2
              i32.add
              local.set $p
              local.get $c
              i32.const -1
              i32.add
              local.tee $c
              br_if $again
            end
            local.get $p
            local.get $c
            i32.const 8
            i32.load16_u
            i32.const 10
            i32.load16_u
            i32.const 12
            i32.load16_u)
          (func (export "sub_loop") (param $ptr i32) (param $scalar i32) (param $count i32)
            (result i32 i32 i32 i32 i32 i32)
            (local $p i32)
            (local $c i32)
            local.get $ptr
            local.set $p
            local.get $count
            local.set $c
            loop $again
              local.get $p
              i32.const 2
              i32.add
              local.get $p
              i32.load16_u
              local.get $scalar
              i32.sub
              i32.store16
              local.get $p
              i32.const 2
              i32.add
              local.set $p
              local.get $c
              i32.const -1
              i32.add
              local.tee $c
              br_if $again
            end
            local.get $p
            local.get $c
            i32.const 32
            i32.load16_u
            i32.const 34
            i32.load16_u
            i32.const 36
            i32.load16_u
            i32.const 38
            i32.load16_u))
        "#,
        &store,
        &registry,
    )
    .await;

    let add = run_module_function(
        &instance,
        &store,
        "add_loop",
        &ResultValue::new(vec![
            WasmValue::I32(8),
            WasmValue::I32(3),
            WasmValue::I32(3),
        ]),
    )
    .await;
    match add {
        VMResult::Success(values) => {
            assert_eq!(
                values,
                ResultValue::new(vec![
                    WasmValue::I32(14),
                    WasmValue::I32(0),
                    WasmValue::I32(4),
                    WasmValue::I32(5),
                    WasmValue::I32(2)
                ])
            );
        }
        other => panic!("load16_u update store16 add loop must succeed, got {other:?}"),
    }

    let sub = run_module_function(
        &instance,
        &store,
        "sub_loop",
        &ResultValue::new(vec![
            WasmValue::I32(32),
            WasmValue::I32(4),
            WasmValue::I32(3),
        ]),
    )
    .await;
    match sub {
        VMResult::Success(values) => {
            assert_eq!(
                values,
                ResultValue::new(vec![
                    WasmValue::I32(38),
                    WasmValue::I32(0),
                    WasmValue::I32(10),
                    WasmValue::I32(6),
                    WasmValue::I32(2),
                    WasmValue::I32(65534)
                ])
            );
        }
        other => panic!("load16_u update store16 sub loop must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_load8_set_update_br_if_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data (i32.const 8) "\03\00")
          (func (export "scan") (param $ptr i32) (result i32 i32)
            (local $p i32)
            (local $next i32)
            (local $byte i32)
            local.get $ptr
            local.set $p
            local.get $ptr
            i32.const 1
            i32.add
            local.set $next
            loop $again
              local.get $p
              i32.load8_u
              local.set $byte
              local.get $next
              local.set $p
              local.get $byte
              br_if $again
            end
            local.get $p
            local.get $byte))
        "#,
        &store,
        &registry,
    )
    .await;

    let result = run_module_function(
        &instance,
        &store,
        "scan",
        &ResultValue::new(vec![WasmValue::I32(8)]),
    )
    .await;
    match result {
        VMResult::Success(values) => {
            assert_eq!(
                values,
                ResultValue::new(vec![WasmValue::I32(9), WasmValue::I32(0)])
            );
        }
        other => panic!("load8 update br_if fusion must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_inc_load8_set_update_br_if_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data (i32.const 8) "\01\00")
          (func (export "scan") (param $base i32) (param $ptr i32) (result i32 i32 i32)
            (local $p i32)
            (local $next i32)
            (local $byte i32)
            local.get $base
            i32.const 0
            i32.store offset=4
            local.get $ptr
            local.set $p
            local.get $ptr
            i32.const 1
            i32.add
            local.set $next
            loop $again
              local.get $base
              local.get $base
              i32.load offset=4
              i32.const 1
              i32.add
              i32.store offset=4
              local.get $p
              i32.load8_u
              local.set $byte
              local.get $next
              local.set $p
              local.get $byte
              br_if $again
            end
            local.get $base
            i32.load offset=4
            local.get $p
            local.get $byte))
        "#,
        &store,
        &registry,
    )
    .await;

    let result = run_module_function(
        &instance,
        &store,
        "scan",
        &ResultValue::new(vec![WasmValue::I32(0), WasmValue::I32(8)]),
    )
    .await;
    match result {
        VMResult::Success(values) => {
            assert_eq!(
                values,
                ResultValue::new(vec![
                    WasmValue::I32(2),
                    WasmValue::I32(9),
                    WasmValue::I32(0)
                ])
            );
        }
        other => panic!("inc load8 update br_if fusion must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_i32_sum_clip_local_base_loop_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data (i32.const 0) "\01\00\00\00\02\00\00\00\64\00\00\00\03\00\00\00")
          (func (export "sum") (param $ptr i32) (param $clip i32) (param $counter i32)
            (result i32 i32 i32 i32 i32 i32)
            (local $value i32)
            (local $acc i32)
            (local $overflow i32)
            (local $tally i32)
            (local $prev i32)
            loop $again
              i32.const 0
              local.get $ptr
              i32.load
              local.tee $value
              local.get $acc
              i32.add
              local.tee $acc
              local.get $acc
              local.get $clip
              i32.gt_s
              local.tee $overflow
              select
              local.set $acc
              i32.const 10
              local.get $value
              local.get $prev
              i32.gt_s
              local.get $overflow
              select
              local.get $tally
              i32.add
              local.set $tally
              local.get $ptr
              i32.const 4
              i32.add
              local.set $ptr
              local.get $value
              local.set $prev
              local.get $counter
              i32.const -1
              i32.add
              local.tee $counter
              br_if $again
            end
            local.get $ptr
            local.get $counter
            local.get $value
            local.get $acc
            local.get $overflow
            local.get $tally))
        "#,
        &store,
        &registry,
    )
    .await;

    let result = run_module_function(
        &instance,
        &store,
        "sum",
        &ResultValue::new(vec![
            WasmValue::I32(0),
            WasmValue::I32(5),
            WasmValue::I32(4),
        ]),
    )
    .await;
    match result {
        VMResult::Success(values) => {
            assert_eq!(
                values,
                ResultValue::new(vec![
                    WasmValue::I32(16),
                    WasmValue::I32(0),
                    WasmValue::I32(3),
                    WasmValue::I32(3),
                    WasmValue::I32(0),
                    WasmValue::I32(12)
                ])
            );
        }
        other => panic!("i32 sum clip local-base loop must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_i32_load_store_local_base_relink_loop_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data (i32.const 8) "\10\00\00\00")
          (data (i32.const 16) "\18\00\00\00")
          (data (i32.const 24) "\00\00\00\00")
          (func (export "relink") (param $cursor i32) (param $prev i32)
            (result i32 i32 i32 i32 i32 i32)
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
            end
            local.get $cursor
            local.get $current
            local.get $prev
            i32.const 8
            i32.load
            i32.const 16
            i32.load
            i32.const 24
            i32.load))
        "#,
        &store,
        &registry,
    )
    .await;

    let result = run_module_function(
        &instance,
        &store,
        "relink",
        &ResultValue::new(vec![WasmValue::I32(8), WasmValue::I32(0)]),
    )
    .await;
    match result {
        VMResult::Success(values) => {
            assert_eq!(
                values,
                ResultValue::new(vec![
                    WasmValue::I32(0),
                    WasmValue::I32(24),
                    WasmValue::I32(24),
                    WasmValue::I32(0),
                    WasmValue::I32(8),
                    WasmValue::I32(16)
                ])
            );
        }
        other => panic!("local-base relink loop must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_scalar_copy_local_base_run_remains_correct_for_overlap() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data (i32.const 8) "\01\02\03\04\05\06\07\08")
          (func (export "copy") (param $dst i32) (param $src i32) (result i32)
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
            i32.store8
            local.get $dst
            i32.load))
        "#,
        &store,
        &registry,
    )
    .await;

    let result = run_module_function(
        &instance,
        &store,
        "copy",
        &ResultValue::new(vec![WasmValue::I32(9), WasmValue::I32(8)]),
    )
    .await;
    match result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(0x01010101)]));
        }
        other => panic!("scalar copy local-base run must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_i32_inc_local_base_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data (i32.const 8) "\00\00\00\00\29\00\00\00")
          (func (export "inc") (param $base i32) (result i32)
            local.get $base
            local.get $base
            i32.load offset=4
            i32.const 1
            i32.add
            i32.store offset=4
            local.get $base
            i32.load offset=4))
        "#,
        &store,
        &registry,
    )
    .await;

    let result = run_module_function(
        &instance,
        &store,
        "inc",
        &ResultValue::new(vec![WasmValue::I32(8)]),
    )
    .await;
    match result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(42)]));
        }
        other => panic!("i32 inc local-base must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_local_get4_i32_inc_local_base_load8_set4_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data (i32.const 8) "\00\00\00\00\29\00\00\00")
          (data (i32.const 20) "\07")
          (func (export "run") (param $preserved i32) (param $inc_base i32) (param $load_base i32)
            (result i32 i32 i32)
            (local $dst i32)
            local.get $preserved
            local.get $inc_base
            local.get $inc_base
            i32.load offset=4
            i32.const 1
            i32.add
            i32.store offset=4
            local.get $load_base
            i32.load8_u
            local.set $dst
            local.get $dst
            local.get $inc_base
            i32.load offset=4))
        "#,
        &store,
        &registry,
    )
    .await;

    let result = run_module_function(
        &instance,
        &store,
        "run",
        &ResultValue::new(vec![
            WasmValue::I32(99),
            WasmValue::I32(8),
            WasmValue::I32(20),
        ]),
    )
    .await;
    match result {
        VMResult::Success(values) => {
            assert_eq!(
                values,
                ResultValue::new(vec![
                    WasmValue::I32(99),
                    WasmValue::I32(7),
                    WasmValue::I32(42)
                ])
            );
        }
        other => panic!("local-get inc load8-set fusion must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_generic_load_tee_branch_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data (i32.const 8) "\2a\00\00\00\00\00\00\00")
          (global $gbase (mut i32) (i32.const 8))
          (func (export "set_base") (param $base i32)
            local.get $base
            global.set $gbase)
          (func (export "br_if8_u") (result i32)
            (local $dst i32)
            block
              global.get $gbase
              i32.load8_u
              local.tee $dst
              br_if 0
              i32.const 7
              return
            end
            local.get $dst)
          (func (export "eqz_br_if16_s") (result i32)
            (local $dst i32)
            block
              global.get $gbase
              i32.load16_s
              local.tee $dst
              i32.eqz
              br_if 0
              i32.const 7
              return
            end
            local.get $dst))
        "#,
        &store,
        &registry,
    )
    .await;

    for (base, expected_br, expected_eqz) in [(8, 42, 7), (12, 7, 0)] {
        let set_result = run_module_function(
            &instance,
            &store,
            "set_base",
            &ResultValue::new(vec![WasmValue::I32(base)]),
        )
        .await;
        assert!(matches!(set_result, VMResult::Success(values) if values.is_empty()));

        for (name, expected) in [("br_if8_u", expected_br), ("eqz_br_if16_s", expected_eqz)] {
            let result =
                run_module_function(&instance, &store, name, &ResultValue::new(vec![])).await;
            match result {
                VMResult::Success(values) => {
                    assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
                }
                other => panic!("generic load tee branch `{name}` must succeed, got {other:?}"),
            }
        }
    }
}

#[tokio::test]
async fn optimizer_local_const_and_branch_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "and_br_if") (param $value i32) (result i32)
            block
              local.get $value
              i32.const 8
              i32.and
              br_if 0
              i32.const 7
              return
            end
            i32.const 42)
          (func (export "and_eqz_br_if") (param $value i32) (result i32)
            block
              local.get $value
              i32.const 8
              i32.and
              i32.eqz
              br_if 0
              i32.const 7
              return
            end
            i32.const 42))
        "#,
        &store,
        &registry,
    )
    .await;

    for (name, value, expected) in [
        ("and_br_if", 8, 42),
        ("and_br_if", 0, 7),
        ("and_eqz_br_if", 8, 7),
        ("and_eqz_br_if", 0, 42),
    ] {
        let result = run_module_function(
            &instance,
            &store,
            name,
            &ResultValue::new(vec![WasmValue::I32(value)]),
        )
        .await;
        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => panic!("local const-and branch `{name}` must succeed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn optimizer_local_const_and_tee_const_eq_branch_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
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
        &store,
        &registry,
    )
    .await;

    for (value, expected) in [(44, 44), (300, 44), (45, 45)] {
        let result = run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![WasmValue::I32(value)]),
        )
        .await;
        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => panic!("local const-and tee const-eq branch must succeed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn optimizer_local_const_and_compare_branch_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "and_ne") (param $value i32) (result i32)
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
            i32.const 42)
          (func (export "add_and_le_u") (param $value i32) (result i32)
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
        &store,
        &registry,
    )
    .await;

    for (name, value, expected) in [
        ("and_ne", 69, 7),
        ("and_ne", 70, 42),
        ("add_and_le_u", 58, 42),
        ("add_and_le_u", 48, 7),
    ] {
        let result = run_module_function(
            &instance,
            &store,
            name,
            &ResultValue::new(vec![WasmValue::I32(value)]),
        )
        .await;
        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => panic!("local const-and compare branch `{name}` must succeed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn optimizer_local_copy_const_compare_branch_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
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
        &store,
        &registry,
    )
    .await;

    for (copy, flag, expected) in [(7, 1, 7), (7, 2, 107)] {
        let result = run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![WasmValue::I32(copy), WasmValue::I32(flag)]),
        )
        .await;
        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => panic!("local copy const-compare branch must succeed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn optimizer_local_add_set_load8_eqz_branch_tail_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data (i32.const 0) "\00\07")
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
        &store,
        &registry,
    )
    .await;

    for (base, expected) in [(0, 1), (1, 9)] {
        let result = run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![WasmValue::I32(base)]),
        )
        .await;
        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => panic!("local add-set load8 eqz branch tail must succeed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn optimizer_local_base_load_tee_load8_branch_tail_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data (i32.const 0) "\04\00\00\00\00\07\00\00\05\00\00\00")
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
              local.get $byte
              i32.add
              return
            end
            local.get $byte
            i32.const 100
            i32.add))
        "#,
        &store,
        &registry,
    )
    .await;

    for (base, expected) in [(0, 4), (8, 107)] {
        let result = run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![WasmValue::I32(base)]),
        )
        .await;
        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => panic!("local-base load tee load8 branch tail must succeed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn optimizer_local_base_load_local_get4_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data (i32.const 8) "\ff\34\12")
          (func (export "root") (param $base i32) (param $preserved i32) (result i32 i32)
            local.get $base
            i32.load16_u offset=1
            local.get $preserved)
          (func (export "tee") (param $base i32) (param $preserved i32) (result i32 i32 i32)
            (local $dst i32)
            local.get $base
            i32.load8_s
            local.tee $dst
            local.get $preserved
            local.get $dst))
        "#,
        &store,
        &registry,
    )
    .await;

    let root = run_module_function(
        &instance,
        &store,
        "root",
        &ResultValue::new(vec![WasmValue::I32(8), WasmValue::I32(77)]),
    )
    .await;
    match root {
        VMResult::Success(values) => {
            assert_eq!(
                values,
                ResultValue::new(vec![WasmValue::I32(0x1234), WasmValue::I32(77)])
            );
        }
        other => panic!("local-base load local-get root must succeed, got {other:?}"),
    }

    let tee = run_module_function(
        &instance,
        &store,
        "tee",
        &ResultValue::new(vec![WasmValue::I32(8), WasmValue::I32(99)]),
    )
    .await;
    match tee {
        VMResult::Success(values) => {
            assert_eq!(
                values,
                ResultValue::new(vec![
                    WasmValue::I32(-1),
                    WasmValue::I32(99),
                    WasmValue::I32(-1)
                ])
            );
        }
        other => panic!("local-base load local-get tee must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_generic_load_local_get4_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data (i32.const 8) "\ff\ff")
          (global $base i32 (i32.const 8))
          (func (export "run") (param $preserved i32) (result i32 i32)
            global.get $base
            i32.load16_s
            local.get $preserved))
        "#,
        &store,
        &registry,
    )
    .await;

    let result = run_module_function(
        &instance,
        &store,
        "run",
        &ResultValue::new(vec![WasmValue::I32(77)]),
    )
    .await;
    match result {
        VMResult::Success(values) => {
            assert_eq!(
                values,
                ResultValue::new(vec![WasmValue::I32(-1), WasmValue::I32(77)])
            );
        }
        other => panic!("generic load local-get root must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_narrow_local_base_store_local_get4_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (func (export "store8") (param $base i32) (param $value i32) (result i32)
            local.get $base
            local.get $value
            i32.store8
            local.get $base
            i32.load8_u)
          (func (export "store16") (param $base i32) (param $value i32) (result i32)
            local.get $base
            local.get $value
            i32.store16
            local.get $base
            i32.load16_u))
        "#,
        &store,
        &registry,
    )
    .await;

    let store8 = run_module_function(
        &instance,
        &store,
        "store8",
        &ResultValue::new(vec![WasmValue::I32(8), WasmValue::I32(0x1234_5678)]),
    )
    .await;
    match store8 {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(0x78)]));
        }
        other => panic!("local-base store8 local-get must succeed, got {other:?}"),
    }

    let store16 = run_module_function(
        &instance,
        &store,
        "store16",
        &ResultValue::new(vec![WasmValue::I32(16), WasmValue::I32(0x1234_5678)]),
    )
    .await;
    match store16 {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(0x5678)]));
        }
        other => panic!("local-base store16 local-get must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_local_get4_copy_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "copy") (param $src i32) (result i32)
            (local $dst i32)
            local.get $src
            local.set $dst
            local.get $dst)
          (func (export "tee") (param $src i32) (result i32)
            (local $dst i32)
            local.get $src
            local.tee $dst))
        "#,
        &store,
        &registry,
    )
    .await;

    for export in ["copy", "tee"] {
        let result = run_module_function(
            &instance,
            &store,
            export,
            &ResultValue::new(vec![WasmValue::I32(1234)]),
        )
        .await;
        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(1234)]));
            }
            other => panic!("local-get copy `{export}` must succeed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn optimizer_i32_const_copy_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "set") (result i32)
            (local $dst i32)
            i32.const -559038737
            local.set $dst
            local.get $dst)
          (func (export "tee") (result i32)
            (local $dst i32)
            i32.const -559038737
            local.tee $dst))
        "#,
        &store,
        &registry,
    )
    .await;

    for export in ["set", "tee"] {
        let result =
            run_module_function(&instance, &store, export, &ResultValue::new(vec![])).await;
        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(-559038737)]));
            }
            other => panic!("i32.const copy `{export}` must succeed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn optimizer_scaled_index_memory_families_remain_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (func (export "run") (param $idx i32) (param $value i32) (result i32)
            local.get $idx
            i32.const 2
            i32.shl
            i32.const 8
            i32.add
            local.get $value
            i32.store
            local.get $idx
            i32.const 2
            i32.shl
            i32.const 8
            i32.add
            i32.load))
        "#,
        &store,
        &registry,
    )
    .await;

    for ((idx, value), expected) in [((0, 11), 11), ((1, 22), 22), ((3, 37), 37)] {
        let result = run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![WasmValue::I32(idx), WasmValue::I32(value)]),
        )
        .await;

        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => {
                panic!("scaled-index memory family({idx}, {value}) must succeed, got {other:?}")
            }
        }
    }
}

#[tokio::test]
async fn optimizer_full_width_scalar_memory_families_remain_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (memory $m 1)
          (func (export "i64-const-base") (param i64) (result i64)
            i32.const 0
            local.get 0
            i64.store
            i32.const 0
            i64.load)
          (func (export "f32-local-base") (param i32 f32) (result f32)
            local.get 0
            local.get 1
            f32.store
            local.get 0
            f32.load)
          (func (export "f64-indexed-scaled") (param i32 f64) (result f64)
            local.get 0
            i32.const 3
            i32.shl
            i32.const 8
            i32.add
            local.get 1
            f64.store $m
            local.get 0
            i32.const 3
            i32.shl
            i32.const 8
            i32.add
            f64.load $m))
        "#,
        &store,
        &registry,
    )
    .await;

    let i64_result = run_module_function(
        &instance,
        &store,
        "i64-const-base",
        &ResultValue::new(vec![WasmValue::I64(0x1122_3344_5566_7788)]),
    )
    .await;
    match i64_result {
        VMResult::Success(values) => {
            assert_eq!(
                values,
                ResultValue::new(vec![WasmValue::I64(0x1122_3344_5566_7788)])
            );
        }
        other => panic!("full-width i64 const-base path must succeed, got {other:?}"),
    }

    let f32_result = run_module_function(
        &instance,
        &store,
        "f32-local-base",
        &ResultValue::new(vec![WasmValue::I32(16), WasmValue::F32(1.5)]),
    )
    .await;
    match f32_result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::F32(1.5)]));
        }
        other => panic!("full-width f32 local-base path must succeed, got {other:?}"),
    }

    let f64_result = run_module_function(
        &instance,
        &store,
        "f64-indexed-scaled",
        &ResultValue::new(vec![WasmValue::I32(2), WasmValue::F64(6.25)]),
    )
    .await;
    match f64_result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::F64(6.25)]));
        }
        other => panic!("full-width f64 indexed-scaled path must succeed, got {other:?}"),
    }
}

#[cfg(feature = "threads")]
#[tokio::test]
async fn optimizer_full_width_shared_scalar_memory_families_remain_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1 2 shared)
          (func (export "f64-shared-local-base") (param i32 f64) (result f64)
            local.get 0
            local.get 1
            f64.store
            local.get 0
            f64.load))
        "#,
        &store,
        &registry,
    )
    .await;

    let result = run_module_function(
        &instance,
        &store,
        "f64-shared-local-base",
        &ResultValue::new(vec![WasmValue::I32(24), WasmValue::F64(9.5)]),
    )
    .await;
    match result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::F64(9.5)]));
        }
        other => panic!("full-width shared local-base path must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_versioned_local_scaled_index_memory_path_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (func (export "run") (param $base i32) (param $idx i32) (param $value i32) (result i32)
            local.get $base
            local.get $idx
            i32.const 2
            i32.shl
            i32.add
            i32.const 16
            i32.add
            local.get $value
            i32.store

            block (result i32)
              local.get $base
              local.get $idx
              i32.const 2
              i32.shl
              i32.add
              i32.const 16
              i32.add
              br 0
            end
            i32.load))
        "#,
        &store,
        &registry,
    )
    .await;

    for ((base, idx, value), expected) in [((0, 0, 11), 11), ((8, 1, 22), 22), ((24, 3, 37), 37)] {
        let result = run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![
                WasmValue::I32(base),
                WasmValue::I32(idx),
                WasmValue::I32(value),
            ]),
        )
        .await;

        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => panic!(
                "versioned local-scaled-index memory path({base}, {idx}, {value}) must succeed, got {other:?}"
            ),
        }
    }
}

#[cfg(feature = "threads")]
#[tokio::test]
async fn optimizer_shared_memory_local_base_families_remain_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1 2 shared)
          (func (export "run") (param $base i32) (param $value i32) (result i32)
            local.get $base
            i32.const 4
            i32.add
            local.get $value
            i32.store
            local.get $base
            i32.const 4
            i32.add
            i32.load))
        "#,
        &store,
        &registry,
    )
    .await;

    for ((base, value), expected) in [((0, 19), 19), ((8, 27), 27)] {
        let result = run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![WasmValue::I32(base), WasmValue::I32(value)]),
        )
        .await;

        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => panic!(
                "shared local-base memory family({base}, {value}) must succeed, got {other:?}"
            ),
        }
    }
}

#[cfg(feature = "threads")]
#[tokio::test]
async fn optimizer_indexed_shared_scaled_index_memory_families_remain_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (memory $m 1 2 shared)
          (func (export "run") (param $idx i32) (param $value i32) (result i32)
            local.get $idx
            i32.const 2
            i32.shl
            i32.const 8
            i32.add
            local.get $value
            i32.store $m
            local.get $idx
            i32.const 2
            i32.shl
            i32.const 8
            i32.add
            i32.load $m))
        "#,
        &store,
        &registry,
    )
    .await;

    for ((idx, value), expected) in [((0, 31), 31), ((2, 47), 47)] {
        let result = run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![WasmValue::I32(idx), WasmValue::I32(value)]),
        )
        .await;

        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => panic!(
                "indexed shared scaled-index memory family({idx}, {value}) must succeed, got {other:?}"
            ),
        }
    }
}

#[tokio::test]
async fn optimizer_store_value_merge_temp_window_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (global $g (mut i32) (i32.const 7))
          (func (export "run") (param i32) (result i32)
            i32.const 0
            block (result i32)
              local.get 0
              if (result i32)
                global.get $g
              else
                i32.const 5
              end
            end
            i32.store
            i32.const 0
            i32.load))
        "#,
        &store,
        &registry,
    )
    .await;

    for (flag, expected) in [(0, 5), (1, 7)] {
        let result = run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![WasmValue::I32(flag)]),
        )
        .await;

        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => {
                panic!("merge-fed store value temp window({flag}) must succeed, got {other:?}")
            }
        }
    }
}

#[tokio::test]
async fn optimizer_memory_derived_address_root_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data (i32.const 0) "\04\00\00\00\2a\00\00\00")
          (func (export "run") (result i32)
            i32.const 0
            i32.load
            i32.load))
        "#,
        &store,
        &registry,
    )
    .await;

    let result = run_module_function(&instance, &store, "run", &ResultValue::new(vec![])).await;
    match result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(42)]));
        }
        other => panic!("memory-derived address root must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_indexed_memory_derived_address_root_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 0)
          (memory $m 1)
          (data (memory $m) (i32.const 0) "\04\00\00\00\2a\00\00\00")
          (func (export "run-load") (result i32)
            i32.const 0
            i32.load $m
            i32.load $m)
          (func (export "run-store") (result i32)
            i32.const 0
            i32.load $m
            i32.const 7
            i32.store $m
            i32.const 4
            i32.load $m))
        "#,
        &store,
        &registry,
    )
    .await;

    let load_result =
        run_module_function(&instance, &store, "run-load", &ResultValue::new(vec![])).await;
    match load_result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(42)]));
        }
        other => panic!("indexed memory-derived load root must succeed, got {other:?}"),
    }

    let store_result =
        run_module_function(&instance, &store, "run-store", &ResultValue::new(vec![])).await;
    match store_result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(7)]));
        }
        other => panic!("indexed memory-derived store root must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_component_style_record_stores_remain_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (func $alloc (result i32)
            i32.const 32)
          (func (export "run") (param i32 f64 i32 i32) (result i32)
            (local $ret i32)
            call $alloc
            local.set $ret
            local.get $ret
            local.get 0
            i32.store
            local.get $ret
            i32.const 8
            i32.add
            local.get 1
            f64.store
            local.get $ret
            i32.const 16
            i32.add
            local.get 2
            i32.store8
            local.get $ret
            i32.const 20
            i32.add
            local.get 3
            i32.store
            local.get $ret
            i32.load))
        "#,
        &store,
        &registry,
    )
    .await;

    let result = run_module_function(
        &instance,
        &store,
        "run",
        &ResultValue::new(vec![
            WasmValue::I32(32343),
            WasmValue::F64(std::f64::consts::PI),
            WasmValue::I32(0),
            WasmValue::I32(314159265),
        ]),
    )
    .await;
    match result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(32343)]));
        }
        other => panic!("component-style record stores must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_memory_address_trap_sensitive_tree_preserves_traps() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data (i32.const 4) "\2a\00\00\00")
          (func (export "run_ok") (result i32)
            i32.const 8
            i32.const 2
            i32.div_s
            i32.load)
          (func (export "run_trap") (result i32)
            i32.const 1
            i32.const 0
            i32.div_s
            i32.load))
        "#,
        &store,
        &registry,
    )
    .await;

    let ok = run_module_function(&instance, &store, "run_ok", &ResultValue::new(vec![])).await;
    match ok {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(42)]));
        }
        other => panic!("trap-sensitive address ok path must succeed, got {other:?}"),
    }

    let trapped =
        run_module_function(&instance, &store, "run_trap", &ResultValue::new(vec![])).await;
    assert!(matches!(trapped, VMResult::InvalidOperand));
}

#[tokio::test]
async fn optimizer_memory_address_non_adjacent_base_plus_const_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data (i32.const 8) "\2a\00\00\00\39\00\00\00")
          (func (export "run_add") (param i32) (result i32)
            local.get 0
            i32.const 8
            i32.add
            i32.const 1
            drop
            i32.load)
          (func (export "run_sub_store") (param i32) (param i32) (result i32)
            local.get 0
            i32.const 4
            i32.add
            local.get 1
            i32.store
            local.get 0
            i32.const 4
            i32.add
            i32.load))
        "#,
        &store,
        &registry,
    )
    .await;

    let add = run_module_function(
        &instance,
        &store,
        "run_add",
        &ResultValue::new(vec![WasmValue::I32(0)]),
    )
    .await;
    match add {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(42)]));
        }
        other => panic!("non-adjacent base+const load must succeed, got {other:?}"),
    }

    let store_then_load = run_module_function(
        &instance,
        &store,
        "run_sub_store",
        &ResultValue::new(vec![WasmValue::I32(8), WasmValue::I32(77)]),
    )
    .await;
    match store_then_load {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(77)]));
        }
        other => panic!("non-adjacent base+const store/load must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_direct_call_neighboring_select_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func $pair (param i32 i32) (result i32)
            local.get 0
            local.get 1
            i32.add)
          (func (export "run") (param i32 i32) (result i32)
            local.get 0
            local.get 1
            local.get 0
            select
            i32.const 5
            call $pair))
        "#,
        &store,
        &registry,
    )
    .await;

    for ((lhs, rhs), expected) in [((0, 9), 14), ((1, 9), 6), ((7, 3), 12)] {
        let result = run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![WasmValue::I32(lhs), WasmValue::I32(rhs)]),
        )
        .await;

        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => {
                panic!("select-neighbor direct call({lhs}, {rhs}) must succeed, got {other:?}")
            }
        }
    }
}

#[tokio::test]
async fn optimizer_direct_call_with_unary_binop_tree_args_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func $mix (param i32 i32) (result i32)
            local.get 0
            local.get 1
            i32.add)
          (func (export "run") (param i32 i32) (result i32)
            local.get 0
            i32.popcnt
            i32.const 9
            drop
            local.get 1
            i32.const 2
            i32.add
            i32.clz
            call $mix))
        "#,
        &store,
        &registry,
    )
    .await;

    for ((lhs, rhs), expected) in [((0, 0), 30), ((7, 1), 33), ((15, 3), 33)] {
        let result = run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![WasmValue::I32(lhs), WasmValue::I32(rhs)]),
        )
        .await;

        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => {
                panic!("direct call unary/binop tree({lhs}, {rhs}) must succeed, got {other:?}")
            }
        }
    }
}

#[tokio::test]
async fn optimizer_import_direct_call_with_unary_binop_tree_args_remains_correct() {
    let store = Store::new();
    let mut registry = Registry::new();
    let host = instantiate_wat(
        r#"
        (module
          (func (export "mix") (param i32 i32) (result i32)
            local.get 0
            local.get 1
            i32.add))
        "#,
        &store,
        &registry,
    )
    .await;
    registry.register("host", host);
    let instance = instantiate_wat(
        r#"
        (module
          (import "host" "mix" (func $mix (param i32 i32) (result i32)))
          (func (export "run") (param i32 i32) (result i32)
            local.get 0
            i32.popcnt
            i32.const 9
            drop
            local.get 1
            i32.const 2
            i32.add
            i32.clz
            call $mix))
        "#,
        &store,
        &registry,
    )
    .await;

    for ((lhs, rhs), expected) in [((0, 0), 30), ((7, 1), 33), ((15, 3), 33)] {
        let result = run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![WasmValue::I32(lhs), WasmValue::I32(rhs)]),
        )
        .await;

        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => {
                panic!(
                    "import direct call unary/binop tree({lhs}, {rhs}) must succeed, got {other:?}"
                )
            }
        }
    }
}

#[tokio::test]
async fn optimizer_import_return_call_with_binop_arg_remains_correct() {
    let store = Store::new();
    let mut registry = Registry::new();
    let host = instantiate_wat(
        r#"
        (module
          (func (export "step") (param i32) (result i32)
            local.get 0
            i32.const 1
            i32.add))
        "#,
        &store,
        &registry,
    )
    .await;
    registry.register("host", host);
    let instance = instantiate_wat(
        r#"
        (module
          (import "host" "step" (func $step (param i32) (result i32)))
          (func (export "run") (param i32) (result i32)
            local.get 0
            i32.const 3
            i32.mul
            return_call $step))
        "#,
        &store,
        &registry,
    )
    .await;

    for (input, expected) in [(0, 1), (2, 7), (9, 28)] {
        let result = run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![WasmValue::I32(input)]),
        )
        .await;

        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => {
                panic!("import return_call binop arg({input}) must succeed, got {other:?}")
            }
        }
    }
}

#[tokio::test]
async fn optimizer_direct_call_with_local_tee_eqz_suffix_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func $mix (param i32 i32) (result i32)
            local.get 0
            local.get 1
            i32.add)
          (func (export "run") (param i32 i32 i32) (result i32)
            (local i32)
            local.get 0
            local.get 1
            local.get 2
            select
            local.get 1
            i32.const 1
            i32.add
            local.tee 3
            i32.eqz
            call $mix))
        "#,
        &store,
        &registry,
    )
    .await;

    for ((lhs, rhs, cond), expected) in [
        ((4, -1, 1), 5),
        ((4, 5, 1), 4),
        ((9, -1, 0), 0),
        ((9, 5, 0), 5),
    ] {
        let result = run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![
                WasmValue::I32(lhs),
                WasmValue::I32(rhs),
                WasmValue::I32(cond),
            ]),
        )
        .await;

        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => {
                panic!(
                    "direct call local_tee eqz suffix({lhs}, {rhs}, {cond}) must succeed, got {other:?}"
                )
            }
        }
    }
}

#[tokio::test]
async fn optimizer_import_direct_call_with_local_tee_eqz_suffix_remains_correct() {
    let store = Store::new();
    let mut registry = Registry::new();
    let host = instantiate_wat(
        r#"
        (module
          (func (export "mix") (param i32 i32) (result i32)
            local.get 0
            local.get 1
            i32.add))
        "#,
        &store,
        &registry,
    )
    .await;
    registry.register("host", host);
    let instance = instantiate_wat(
        r#"
        (module
          (import "host" "mix" (func $mix (param i32 i32) (result i32)))
          (func (export "run") (param i32 i32 i32) (result i32)
            (local i32)
            local.get 0
            local.get 1
            local.get 2
            select
            local.get 1
            i32.const 1
            i32.add
            local.tee 3
            i32.eqz
            call $mix))
        "#,
        &store,
        &registry,
    )
    .await;

    for ((lhs, rhs, cond), expected) in [
        ((4, -1, 1), 5),
        ((4, 5, 1), 4),
        ((9, -1, 0), 0),
        ((9, 5, 0), 5),
    ] {
        let result = run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![
                WasmValue::I32(lhs),
                WasmValue::I32(rhs),
                WasmValue::I32(cond),
            ]),
        )
        .await;

        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => {
                panic!(
                    "import direct call local_tee eqz suffix({lhs}, {rhs}, {cond}) must succeed, got {other:?}"
                )
            }
        }
    }
}

#[tokio::test]
async fn optimizer_select_with_local_tee_rhs_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "run") (param i32 i32) (result i32)
            (select
              (local.get 0)
              (local.tee 0 (i32.const 6))
              (local.get 1)
            )))
        "#,
        &store,
        &registry,
    )
    .await;

    let result = run_module_function(
        &instance,
        &store,
        "run",
        &ResultValue::new(vec![WasmValue::I32(0), WasmValue::I32(0)]),
    )
    .await;

    match result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(6)]));
        }
        other => panic!("select with local.tee rhs must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_local_tee_wast_module_select_second_remains_correct() {
    let wast = include_str!("wasm-testsuite/local_tee.wast");
    let (module, _) = wast
        .split_once("\n)\n\n(assert")
        .expect("local_tee.wast must contain a module followed by asserts");
    let module = format!("{module}\n)");
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(&module, &store, &registry).await;

    let result = run_module_function(
        &instance,
        &store,
        "as-select-second",
        &ResultValue::new(vec![WasmValue::I32(0), WasmValue::I32(0)]),
    )
    .await;

    match result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(6)]));
        }
        other => panic!("full local_tee module select must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_local_tee_select_calls_do_not_leak_frame_state() {
    let wast = include_str!("wasm-testsuite/local_tee.wast");
    let (module, _) = wast
        .split_once("\n)\n\n(assert")
        .expect("local_tee.wast must contain a module followed by asserts");
    let module = format!("{module}\n)");
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(&module, &store, &registry).await;

    let first = run_module_function(
        &instance,
        &store,
        "as-select-first",
        &ResultValue::new(vec![WasmValue::I32(0), WasmValue::I32(1)]),
    )
    .await;
    match first {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(5)]));
        }
        other => panic!("first select call must succeed, got {other:?}"),
    }

    let second = run_module_function(
        &instance,
        &store,
        "as-select-second",
        &ResultValue::new(vec![WasmValue::I32(0), WasmValue::I32(0)]),
    )
    .await;
    match second {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(6)]));
        }
        other => panic!("second select call must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_local_tee_control_prefix_before_select_second_remains_correct() {
    let wast = include_str!("wasm-testsuite/local_tee.wast");
    let (module, _) = wast
        .split_once("\n)\n\n(assert")
        .expect("local_tee.wast must contain a module followed by asserts");
    let module = format!("{module}\n)");
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(&module, &store, &registry).await;

    let calls = [
        (
            "as-if-cond",
            vec![WasmValue::I32(0)],
            vec![WasmValue::I32(0)],
        ),
        (
            "as-if-then",
            vec![WasmValue::I32(1)],
            vec![WasmValue::I32(3)],
        ),
        (
            "as-if-else",
            vec![WasmValue::I32(0)],
            vec![WasmValue::I32(4)],
        ),
        (
            "as-select-first",
            vec![WasmValue::I32(0), WasmValue::I32(1)],
            vec![WasmValue::I32(5)],
        ),
    ];

    for (name, args, expected) in calls {
        let result = run_module_function(&instance, &store, name, &ResultValue::new(args)).await;
        match result {
            VMResult::Success(values) => assert_eq!(values, ResultValue::new(expected)),
            other => panic!("{name} must succeed, got {other:?}"),
        }
    }

    let second = run_module_function(
        &instance,
        &store,
        "as-select-second",
        &ResultValue::new(vec![WasmValue::I32(0), WasmValue::I32(0)]),
    )
    .await;
    match second {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(6)]));
        }
        other => panic!("second select call must succeed after control prefix, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_local_tee_select_second_before_select_cond_remains_correct() {
    let wast = include_str!("wasm-testsuite/local_tee.wast");
    let (module, _) = wast
        .split_once("\n)\n\n(assert")
        .expect("local_tee.wast must contain a module followed by asserts");
    let module = format!("{module}\n)");
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(&module, &store, &registry).await;

    let second = run_module_function(
        &instance,
        &store,
        "as-select-second",
        &ResultValue::new(vec![WasmValue::I32(0), WasmValue::I32(0)]),
    )
    .await;
    match second {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(6)]));
        }
        other => panic!("second select call must succeed, got {other:?}"),
    }

    let cond = run_module_function(
        &instance,
        &store,
        "as-select-cond",
        &ResultValue::new(vec![WasmValue::I32(0)]),
    )
    .await;
    match cond {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(0)]));
        }
        other => panic!("select-cond must succeed after select-second, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_local_tee_select_cond_alone_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "run") (param i32) (result i32)
            (select
              (i32.const 0)
              (i32.const 1)
              (local.tee 0 (i32.const 7))
            )))
        "#,
        &store,
        &registry,
    )
    .await;

    let result = run_module_function(
        &instance,
        &store,
        "run",
        &ResultValue::new(vec![WasmValue::I32(0)]),
    )
    .await;

    match result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(0)]));
        }
        other => panic!("select-cond alone must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_full_local_tee_module_select_cond_alone_remains_correct() {
    let wast = include_str!("wasm-testsuite/local_tee.wast");
    let (module, _) = wast
        .split_once("\n)\n\n(assert")
        .expect("local_tee.wast must contain a module followed by asserts");
    let module = format!("{module}\n)");
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(&module, &store, &registry).await;

    let result = run_module_function(
        &instance,
        &store,
        "as-select-cond",
        &ResultValue::new(vec![WasmValue::I32(0)]),
    )
    .await;

    match result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(0)]));
        }
        other => panic!("full local_tee module select-cond must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_full_local_tee_module_binary_right_remains_correct() {
    let wast = include_str!("wasm-testsuite/local_tee.wast");
    let (module, _) = wast
        .split_once("\n)\n\n(assert")
        .expect("local_tee.wast must contain a module followed by asserts");
    let module = format!("{module}\n)");
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(&module, &store, &registry).await;

    let result = run_module_function(
        &instance,
        &store,
        "as-binary-right",
        &ResultValue::new(vec![WasmValue::I32(0)]),
    )
    .await;

    match result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(6)]));
        }
        other => panic!("full local_tee module binary-right must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_preserves_i64_div_u_trap_under_drop() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "run") (param i64 i64)
            (drop (i64.div_u (local.get 0) (local.get 1)))))
        "#,
        &store,
        &registry,
    )
    .await;

    let result = run_module_function(
        &instance,
        &store,
        "run",
        &ResultValue::new(vec![WasmValue::I64(1), WasmValue::I64(0)]),
    )
    .await;

    assert!(result.is_err(), "dropped i64.div_u must still trap");
}

#[tokio::test]
async fn optimizer_preserves_i32_div_s_overflow_trap_under_drop() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "run") (param i32 i32)
            (drop (i32.div_s (local.get 0) (local.get 1)))))
        "#,
        &store,
        &registry,
    )
    .await;

    let result = run_module_function(
        &instance,
        &store,
        "run",
        &ResultValue::new(vec![WasmValue::I32(i32::MIN), WasmValue::I32(-1)]),
    )
    .await;

    assert!(
        result.is_err(),
        "dropped i32.div_s overflow must still trap"
    );
}

#[tokio::test]
async fn optimizer_preserves_full_traps_module_i64_div_u_trap_under_drop() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
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
        &store,
        &registry,
    )
    .await;

    for (name, args) in [
        (
            "no_dce.i32.div_s",
            ResultValue::new(vec![WasmValue::I32(1), WasmValue::I32(0)]),
        ),
        (
            "no_dce.i32.div_u",
            ResultValue::new(vec![WasmValue::I32(1), WasmValue::I32(0)]),
        ),
        (
            "no_dce.i64.div_s",
            ResultValue::new(vec![WasmValue::I64(1), WasmValue::I64(0)]),
        ),
        (
            "no_dce.i64.div_u",
            ResultValue::new(vec![WasmValue::I64(1), WasmValue::I64(0)]),
        ),
    ] {
        let result = run_module_function(&instance, &store, name, &args).await;
        assert!(result.is_err(), "{name} must still trap under drop");
    }
}

#[tokio::test]
async fn optimizer_run_wast_traps_first_module_remains_correct() {
    run_wast(
        r#"
        (module
          (func (export "no_dce.i32.div_s") (param $x i32) (param $y i32)
            (drop (i32.div_s (local.get $x) (local.get $y))))
          (func (export "no_dce.i32.div_u") (param $x i32) (param $y i32)
            (drop (i32.div_u (local.get $x) (local.get $y))))
          (func (export "no_dce.i64.div_s") (param $x i64) (param $y i64)
            (drop (i64.div_s (local.get $x) (local.get $y))))
          (func (export "no_dce.i64.div_u") (param $x i64) (param $y i64)
            (drop (i64.div_u (local.get $x) (local.get $y)))))

        (assert_trap (invoke "no_dce.i32.div_s" (i32.const 1) (i32.const 0)) "integer divide by zero")
        (assert_trap (invoke "no_dce.i32.div_u" (i32.const 1) (i32.const 0)) "integer divide by zero")
        (assert_trap (invoke "no_dce.i64.div_s" (i64.const 1) (i64.const 0)) "integer divide by zero")
        (assert_trap (invoke "no_dce.i64.div_u" (i64.const 1) (i64.const 0)) "integer divide by zero")
        "#,
    )
    .await;
}

#[tokio::test]
async fn optimizer_run_wast_full_traps_fixture_remains_correct() {
    run_wast(include_str!("wasm-testsuite/traps.wast")).await;
}

#[tokio::test]
async fn optimizer_local_tee_prefix_up_to_select_cond_remains_correct() {
    let wast = include_str!("wasm-testsuite/local_tee.wast");
    let (module, _) = wast
        .split_once("\n)\n\n(assert")
        .expect("local_tee.wast must contain a module followed by asserts");
    let module = format!("{module}\n)");
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(&module, &store, &registry).await;

    let calls = vec![
        (
            "as-block-mid",
            vec![WasmValue::I32(0)],
            vec![WasmValue::I32(1)],
        ),
        (
            "as-block-last",
            vec![WasmValue::I32(0)],
            vec![WasmValue::I32(1)],
        ),
        (
            "as-loop-first",
            vec![WasmValue::I32(0)],
            vec![WasmValue::I32(3)],
        ),
        (
            "as-loop-mid",
            vec![WasmValue::I32(0)],
            vec![WasmValue::I32(4)],
        ),
        (
            "as-loop-last",
            vec![WasmValue::I32(0)],
            vec![WasmValue::I32(5)],
        ),
        (
            "as-br-value",
            vec![WasmValue::I32(0)],
            vec![WasmValue::I32(9)],
        ),
        ("as-br_if-cond", vec![WasmValue::I32(0)], vec![]),
        (
            "as-br_if-value",
            vec![WasmValue::I32(0)],
            vec![WasmValue::I32(8)],
        ),
        (
            "as-br_if-value-cond",
            vec![WasmValue::I32(0)],
            vec![WasmValue::I32(6)],
        ),
        ("as-br_table-index", vec![WasmValue::I32(0)], vec![]),
        (
            "as-br_table-value",
            vec![WasmValue::I32(0)],
            vec![WasmValue::I32(10)],
        ),
        (
            "as-br_table-value-index",
            vec![WasmValue::I32(0)],
            vec![WasmValue::I32(6)],
        ),
        (
            "as-return-value",
            vec![WasmValue::I32(0)],
            vec![WasmValue::I32(7)],
        ),
        (
            "as-if-cond",
            vec![WasmValue::I32(0)],
            vec![WasmValue::I32(0)],
        ),
        (
            "as-if-then",
            vec![WasmValue::I32(1)],
            vec![WasmValue::I32(3)],
        ),
        (
            "as-if-else",
            vec![WasmValue::I32(0)],
            vec![WasmValue::I32(4)],
        ),
        (
            "as-select-first",
            vec![WasmValue::I32(0), WasmValue::I32(1)],
            vec![WasmValue::I32(5)],
        ),
        (
            "as-select-second",
            vec![WasmValue::I32(0), WasmValue::I32(0)],
            vec![WasmValue::I32(6)],
        ),
        (
            "as-select-cond",
            vec![WasmValue::I32(0)],
            vec![WasmValue::I32(0)],
        ),
    ];

    for (name, args, expected) in calls {
        let result = run_module_function(&instance, &store, name, &ResultValue::new(args)).await;
        match result {
            VMResult::Success(values) => assert_eq!(values, ResultValue::new(expected), "{name}"),
            other => panic!("{name} must succeed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn optimizer_float_xkcd_sqrt_5_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "run") (param f32) (param f32) (param f32) (result f32)
            (f32.add
              (f32.div (local.get 0) (local.get 1))
              (f32.div (local.get 2) (local.get 0))
            )))
        "#,
        &store,
        &registry,
    )
    .await;

    let result = run_module_function(
        &instance,
        &store,
        "run",
        &ResultValue::new(vec![
            WasmValue::F32(2.0),
            WasmValue::F32(2.7182817),
            WasmValue::F32(3.0),
        ]),
    )
    .await;

    match result {
        VMResult::Success(values) => match values.iter().next() {
            Some(WasmValue::F32(actual)) => {
                assert_eq!(actual.to_bits(), 0x400f_16ac);
            }
            other => panic!("float run must return one f32, got {other:?}"),
        },
        other => panic!("float xkcd sqrt 5 must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_full_float_exprs_xkcd_sqrt_5_remains_correct() {
    let wast = include_str!("wasm-testsuite/float_exprs.wast");
    let start = wast
        .find("(module\n  (func (export \"f32.sqrt\")")
        .expect("float_exprs.wast must contain sqrt approximation module");
    let rest = &wast[start..];
    let (module, _) = rest
        .split_once("\n)\n\n(assert_return (invoke \"f32.sqrt\"")
        .expect("sqrt approximation module must be followed by asserts");
    let module = format!("{module}\n)");

    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(&module, &store, &registry).await;

    let result = run_module_function(
        &instance,
        &store,
        "f32.xkcd_sqrt_5",
        &ResultValue::new(vec![
            WasmValue::F32(2.0),
            WasmValue::F32(2.7182817),
            WasmValue::F32(3.0),
        ]),
    )
    .await;

    match result {
        VMResult::Success(values) => match values.iter().next() {
            Some(WasmValue::F32(actual)) => {
                assert_eq!(actual.to_bits(), 0x400f_16ac);
            }
            other => panic!("full float module must return one f32, got {other:?}"),
        },
        other => panic!("full float module xkcd sqrt 5 must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_run_wast_local_tee_fixture_remains_correct() {
    run_wast(include_str!("wasm-testsuite/local_tee.wast")).await;
}

#[tokio::test]
async fn optimizer_recursive_fib_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
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
        &store,
        &registry,
    )
    .await;

    let cases: &[(i32, i32)] = if cfg!(debug_assertions) {
        &[(0, 0), (1, 1), (2, 1), (5, 5), (8, 21)]
    } else {
        &[(0, 0), (1, 1), (2, 1), (5, 5), (8, 21), (10, 55), (12, 144)]
    };
    for (input, expected) in cases {
        let result = run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![WasmValue::I32(*input)]),
        )
        .await;

        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(*expected)]));
            }
            other => panic!("recursive fib({input}) must succeed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn optimizer_tail_recursive_return_call_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "run") (param i32) (result i32)
            local.get 0
            i32.const 0
            call 1)
          (func (param $n i32) (param $acc i32) (result i32)
            local.get $n
            i32.eqz
            if
              local.get $acc
              return
            end
            local.get $n
            i32.const 1
            i32.sub
            local.get $acc
            local.get $n
            i32.add
            return_call 1))
        "#,
        &store,
        &registry,
    )
    .await;

    for (input, expected) in [(0, 0), (1, 1), (5, 15), (16, 136)] {
        let result = run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![WasmValue::I32(input)]),
        )
        .await;

        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => panic!("tail-recursive sum({input}) must succeed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn optimizer_tail_recursive_return_call_with_const_accumulator_step_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "run") (param i32) (result i32)
            local.get 0
            i32.const 0
            call 1)
          (func (param $n i32) (param $acc i32) (result i32)
            local.get $n
            i32.eqz
            if
              local.get $acc
              return
            end
            local.get $n
            i32.const 1
            i32.sub
            local.get $acc
            i32.const 2
            i32.add
            return_call 1))
        "#,
        &store,
        &registry,
    )
    .await;

    for (input, expected) in [(0, 0), (1, 2), (5, 10), (16, 32)] {
        let result = run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![WasmValue::I32(input)]),
        )
        .await;

        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => panic!("tail-recursive const-step sum({input}) must succeed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn optimizer_br_if_families_remain_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "direct") (param i32) (result i32)
            block $done
              local.get 0
              br_if $done
              i32.const 11
              return
            end
            i32.const 22)

          (func (export "add") (param i32) (result i32)
            block $done
              local.get 0
              i32.const 1
              i32.add
              br_if $done
              i32.const 11
              return
            end
            i32.const 22)

          (func (export "local_add") (param i32) (param i32) (result i32)
            block $done
              local.get 0
              local.get 1
              i32.add
              br_if $done
              i32.const 11
              return
            end
            i32.const 22)

          (func (export "eqz") (param i32) (result i32)
            block $done
              local.get 0
              i32.eqz
              br_if $done
              i32.const 11
              return
            end
            i32.const 22)

          (func (export "const_cmp") (param i32) (result i32)
            block $done
              local.get 0
              i32.const 7
              i32.eq
              br_if $done
              i32.const 11
              return
            end
            i32.const 22)

          (func (export "local_cmp") (param i32) (param i32) (result i32)
            block $done
              local.get 0
              local.get 1
              i32.lt_s
              br_if $done
              i32.const 11
              return
            end
            i32.const 22)

          (func (export "tee_add") (param i32) (result i32)
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
        &store,
        &registry,
    )
    .await;

    for (name, args, expected) in [
        ("direct", vec![WasmValue::I32(0)], 11),
        ("direct", vec![WasmValue::I32(1)], 22),
        ("add", vec![WasmValue::I32(0)], 22),
        ("add", vec![WasmValue::I32(-1)], 11),
        ("local_add", vec![WasmValue::I32(0), WasmValue::I32(0)], 11),
        ("local_add", vec![WasmValue::I32(1), WasmValue::I32(2)], 22),
        ("eqz", vec![WasmValue::I32(0)], 22),
        ("eqz", vec![WasmValue::I32(1)], 11),
        ("const_cmp", vec![WasmValue::I32(7)], 22),
        ("const_cmp", vec![WasmValue::I32(1)], 11),
        ("local_cmp", vec![WasmValue::I32(1), WasmValue::I32(2)], 22),
        ("local_cmp", vec![WasmValue::I32(2), WasmValue::I32(1)], 11),
        ("tee_add", vec![WasmValue::I32(-1)], 0),
        ("tee_add", vec![WasmValue::I32(1)], 2),
    ] {
        let result = run_module_function(&instance, &store, name, &ResultValue::new(args)).await;
        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => panic!("{name} must succeed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn optimizer_spill_reused_br_if_families_remain_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data (i32.const 0) "*\00\00\00")
          (global $g (mut i32) (i32.const 1))

          (func (export "global_add") (result i32)
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
            i32.const 22)

          (func (export "load_direct") (result i32)
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
        &store,
        &registry,
    )
    .await;

    for (name, expected) in [("global_add", 22), ("load_direct", 22)] {
        let result = run_module_function(&instance, &store, name, &ResultValue::new(vec![])).await;
        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => panic!("{name} must succeed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn optimizer_numeric_local_binop_families_remain_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "i32_sub") (param i32) (result i32)
            local.get 0
            i32.const 1
            i32.sub)

          (func (export "i32_and") (param i32 i32) (result i32)
            local.get 0
            local.get 1
            i32.and)

          (func (export "i64_xor") (param i64 i64) (result i64)
            local.get 0
            local.get 1
            i64.xor)

          (func (export "i64_shr_u") (param i64) (result i64)
            local.get 0
            i64.const 1
            i64.shr_u))
        "#,
        &store,
        &registry,
    )
    .await;

    for (name, args, expected) in [
        (
            "i32_sub",
            vec![WasmValue::I32(9)],
            ResultValue::new(vec![WasmValue::I32(8)]),
        ),
        (
            "i32_and",
            vec![WasmValue::I32(0b1011), WasmValue::I32(0b0110)],
            ResultValue::new(vec![WasmValue::I32(0b0010)]),
        ),
        (
            "i64_xor",
            vec![WasmValue::I64(0b1010), WasmValue::I64(0b1100)],
            ResultValue::new(vec![WasmValue::I64(0b0110)]),
        ),
        (
            "i64_shr_u",
            vec![WasmValue::I64(8)],
            ResultValue::new(vec![WasmValue::I64(4)]),
        ),
    ] {
        let result = run_module_function(&instance, &store, name, &ResultValue::new(args)).await;
        match result {
            VMResult::Success(values) => assert_eq!(values, expected),
            other => panic!("{name} must succeed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn optimizer_numeric_local_float_and_compare_families_remain_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "f32_mul") (param f32) (result f32)
            local.get 0
            f32.const 1.5
            f32.mul)

          (func (export "f64_div") (param f64 f64) (result f64)
            local.get 0
            local.get 1
            f64.div)

          (func (export "f32_nan_eq") (param f32 f32) (result i32)
            local.get 0
            local.get 1
            f32.eq)

          (func (export "f32_nan_ne") (param f32 f32) (result i32)
            local.get 0
            local.get 1
            f32.ne)

          (func (export "f64_lt_br_if") (param f64 f64) (result i32)
            block $done
              local.get 0
              local.get 1
              f64.lt
              br_if $done
              i32.const 11
              return
            end
            i32.const 22))
        "#,
        &store,
        &registry,
    )
    .await;

    for (name, args, expected) in [
        (
            "f32_mul",
            vec![WasmValue::F32(3.0)],
            ResultValue::new(vec![WasmValue::F32(4.5)]),
        ),
        (
            "f64_div",
            vec![WasmValue::F64(9.0), WasmValue::F64(2.0)],
            ResultValue::new(vec![WasmValue::F64(4.5)]),
        ),
        (
            "f32_nan_eq",
            vec![WasmValue::F32(f32::NAN), WasmValue::F32(1.0)],
            ResultValue::new(vec![WasmValue::I32(0)]),
        ),
        (
            "f32_nan_ne",
            vec![WasmValue::F32(f32::NAN), WasmValue::F32(1.0)],
            ResultValue::new(vec![WasmValue::I32(1)]),
        ),
        (
            "f64_lt_br_if",
            vec![WasmValue::F64(1.0), WasmValue::F64(2.0)],
            ResultValue::new(vec![WasmValue::I32(22)]),
        ),
        (
            "f64_lt_br_if",
            vec![WasmValue::F64(2.0), WasmValue::F64(1.0)],
            ResultValue::new(vec![WasmValue::I32(11)]),
        ),
    ] {
        let result = run_module_function(&instance, &store, name, &ResultValue::new(args)).await;
        match result {
            VMResult::Success(values) => assert_eq!(values, expected),
            other => panic!("{name} must succeed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn optimizer_unary_local_families_remain_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "i32_clz") (param i32) (result i32)
            local.get 0
            i32.clz)

          (func (export "i64_popcnt") (param i64) (result i64)
            local.get 0
            i64.popcnt)

          (func (export "f32_neg") (param f32) (result f32)
            local.get 0
            f32.neg)

          (func (export "f32_nearest") (param f32) (result f32)
            local.get 0
            f32.nearest)

          (func (export "f64_sqrt") (param f64) (result f64)
            local.get 0
            f64.sqrt)

          (func (export "f64_nearest") (param f64) (result f64)
            local.get 0
            f64.nearest))
        "#,
        &store,
        &registry,
    )
    .await;

    for (name, args, expected) in [
        (
            "i32_clz",
            vec![WasmValue::I32(0b0001_0000)],
            ResultValue::new(vec![WasmValue::I32(27)]),
        ),
        (
            "i64_popcnt",
            vec![WasmValue::I64(0b1011_0001)],
            ResultValue::new(vec![WasmValue::I64(4)]),
        ),
        (
            "f32_neg",
            vec![WasmValue::F32(0.0)],
            ResultValue::new(vec![WasmValue::F32(-0.0)]),
        ),
        (
            "f32_nearest",
            vec![WasmValue::F32(2.5)],
            ResultValue::new(vec![WasmValue::F32(2.0)]),
        ),
        (
            "f64_sqrt",
            vec![WasmValue::F64(9.0)],
            ResultValue::new(vec![WasmValue::F64(3.0)]),
        ),
    ] {
        let result = run_module_function(&instance, &store, name, &ResultValue::new(args)).await;
        match result {
            VMResult::Success(values) => assert_eq!(values, expected),
            other => panic!("{name} must succeed, got {other:?}"),
        }
    }

    let neg_zero = run_module_function(
        &instance,
        &store,
        "f32_neg",
        &ResultValue::new(vec![WasmValue::F32(0.0)]),
    )
    .await;
    match neg_zero {
        VMResult::Success(values) => match values.iter().next() {
            Some(WasmValue::F32(value)) => assert_eq!(value.to_bits(), (-0.0f32).to_bits()),
            other => panic!("unexpected result: {other:?}"),
        },
        other => panic!("f32_neg must succeed, got {other:?}"),
    }

    let nearest_nan = run_module_function(
        &instance,
        &store,
        "f64_nearest",
        &ResultValue::new(vec![WasmValue::F64(f64::NAN)]),
    )
    .await;
    match nearest_nan {
        VMResult::Success(values) => match values.iter().next() {
            Some(WasmValue::F64(value)) => assert!(value.is_nan()),
            other => panic!("unexpected result: {other:?}"),
        },
        other => panic!("f64_nearest must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_residual_address_shape_memory_families_remain_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)

          (func (export "block_arg_load") (param i32) (result i32)
            local.get 0
            i32.const 7
            i32.store offset=4
            block (result i32)
              local.get 0
              br 0
            end
            i32.load offset=4)

          (func (export "block_arg_store_load") (param i32 i32) (result i32)
            block (result i32)
              local.get 0
              br 0
            end
            local.get 1
            i32.store offset=8
            local.get 0
            i32.load offset=8))
        "#,
        &store,
        &registry,
    )
    .await;

    for (name, args, expected) in [
        (
            "block_arg_load",
            vec![WasmValue::I32(0)],
            ResultValue::new(vec![WasmValue::I32(7)]),
        ),
        (
            "block_arg_store_load",
            vec![WasmValue::I32(16), WasmValue::I32(99)],
            ResultValue::new(vec![WasmValue::I32(99)]),
        ),
    ] {
        let result = run_module_function(&instance, &store, name, &ResultValue::new(args)).await;
        match result {
            VMResult::Success(values) => assert_eq!(values, expected),
            other => panic!("{name} must succeed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn optimizer_licm_preparation_hoists_remain_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data (i32.const 9) "*")
          (global $g (mut i32) (i32.const 8))

          (func (export "addr_loop") (param $n i32) (result i32)
            (local $acc i32)
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
            local.get $acc)

          (func (export "cmp_loop") (param $n i32) (result i32)
            block $done
              loop $loop
                global.get $g
                i32.const 8
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
        &store,
        &registry,
    )
    .await;

    for (name, args, expected) in [
        ("addr_loop", vec![WasmValue::I32(3)], 42),
        ("addr_loop", vec![WasmValue::I32(0)], 42),
        ("cmp_loop", vec![WasmValue::I32(5)], 5),
    ] {
        let result = run_module_function(&instance, &store, name, &ResultValue::new(args)).await;
        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => panic!("{name} must succeed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn optimizer_store_specializations_remain_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (global $g (mut i32) (i32.const 7))

          (func (export "write_direct") (param $base i32) (param $lhs i32) (param $rhs i32)
            local.get $base
            local.get $lhs
            local.get $rhs
            i32.add
            i32.store
          )

          (func (export "read_direct") (param $base i32) (result i32)
            local.get $base
            i32.load)

          (func (export "write_offset") (param $base i32) (param $lhs i32) (param $rhs i32)
            local.get $base
            i32.const 5
            i32.add
            local.get $lhs
            local.get $rhs
            i32.add
            i32.store8
          )

          (func (export "read_offset") (param $base i32) (result i32)
            local.get $base
            i32.const 5
            i32.add
            i32.load8_u)

          (func (export "write_spill") (param $lhs i32) (param $rhs i32)
            global.get $g
            drop
            global.get $g
            i32.const 1
            i32.add
            local.get $lhs
            local.get $rhs
            i32.add
            i32.store8
          )

          (func (export "read_spill") (result i32)
            global.get $g
            i32.const 1
            i32.add
            i32.load8_u))
        "#,
        &store,
        &registry,
    )
    .await;

    for (write_name, write_args, read_name, read_args, expected) in [
        (
            "write_direct",
            vec![WasmValue::I32(0), WasmValue::I32(19), WasmValue::I32(23)],
            "read_direct",
            vec![WasmValue::I32(0)],
            42,
        ),
        (
            "write_offset",
            vec![WasmValue::I32(8), WasmValue::I32(19), WasmValue::I32(23)],
            "read_offset",
            vec![WasmValue::I32(8)],
            42,
        ),
        (
            "write_spill",
            vec![WasmValue::I32(19), WasmValue::I32(23)],
            "read_spill",
            Vec::new(),
            42,
        ),
    ] {
        let write_result =
            run_module_function(&instance, &store, write_name, &ResultValue::new(write_args)).await;
        match write_result {
            VMResult::Success(values) => {
                assert!(
                    values.is_empty(),
                    "store writer {write_name} must not return values, got {values:?}",
                );
            }
            other => panic!("specialized store writer {write_name} must succeed, got {other:?}"),
        }

        let result =
            run_module_function(&instance, &store, read_name, &ResultValue::new(read_args)).await;

        match result {
            VMResult::Success(values) => {
                assert_eq!(
                    values,
                    ResultValue::new(vec![WasmValue::I32(expected)]),
                    "store specialization case {write_name}/{read_name} returned unexpected value",
                );
            }
            other => panic!("specialized store reader {read_name} must succeed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn optimizer_preserves_multi_value_block_drop_order() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func $dummy)

          (func (export "multi") (result i32)
            (block (call $dummy) (call $dummy) (call $dummy) (call $dummy))
            (block (result i32)
              (call $dummy) (call $dummy) (call $dummy) (i32.const 7) (call $dummy)
            )
            (drop)
            (block (result i32 i64 i32)
              (call $dummy) (call $dummy) (call $dummy) (i32.const 8) (call $dummy)
              (call $dummy) (call $dummy) (call $dummy) (i64.const 7) (call $dummy)
              (call $dummy) (call $dummy) (call $dummy) (i32.const 9) (call $dummy)
            )
            (drop)
            (drop))
        )
        "#,
        &store,
        &registry,
    )
    .await;

    let result = run_module_function(&instance, &store, "multi", &ResultValue::new(vec![])).await;
    match result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(8)]));
        }
        other => panic!("multi-value block case must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_preserves_nested_br_if_value_merges() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "run") (param i32) (result i32)
            (block (result i32)
              (drop
                (br_if 0
                  (i32.const 2)
                  (br_if 0 (i32.const 1) (local.get 0))))
              (i32.const 4)))
        )
        "#,
        &store,
        &registry,
    )
    .await;

    for (input, expected) in [(0, 2), (1, 1)] {
        let result = run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![WasmValue::I32(input)]),
        )
        .await;
        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => panic!("nested br_if merge case {input} must succeed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn optimizer_large_br_table_metadata_remains_representable() {
    let table = std::iter::repeat_n("0", 300).collect::<Vec<_>>().join(" ");
    let wat = format!(
        r#"
        (module
          (func (export "run") (param i32) (result i32)
            (block (result i32)
              (i32.const 42)
              (local.get 0)
              (br_table {table}))))
        "#
    );

    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(&wat, &store, &registry).await;
    let result = run_module_function(
        &instance,
        &store,
        "run",
        &ResultValue::new(vec![WasmValue::I32(0)]),
    )
    .await;

    match result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(42)]));
        }
        other => panic!("large br_table case must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_preserves_loop_result_values_across_side_effects() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func $dummy)

          (func (export "first") (result i32)
            (loop (result i32)
              (block (result i32) (i32.const 1))
              (call $dummy)
              (call $dummy)))

          (func (export "mid") (result i32)
            (loop (result i32)
              (call $dummy)
              (block (result i32) (i32.const 1))
              (call $dummy)))

          (func (export "last") (result i32)
            (loop (result i32)
              (call $dummy)
              (call $dummy)
              (block (result i32) (i32.const 1)))))
        "#,
        &store,
        &registry,
    )
    .await;

    for name in ["first", "mid", "last"] {
        let result = run_module_function(&instance, &store, name, &ResultValue::new(vec![])).await;
        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(1)]));
            }
            other => panic!("{name} loop result case must succeed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn optimizer_preserves_block_values_in_call_indirect_and_control_operands() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func $func (param i32 i32) (result i32) (local.get 0))
          (type $check (func (param i32 i32) (result i32)))
          (table funcref (elem $func))

          (func (export "call_indirect_first") (result i32)
            (block (result i32)
              (call_indirect (type $check)
                (block (result i32) (i32.const 1))
                (i32.const 2)
                (i32.const 0))))

          (func (export "call_indirect_mid") (result i32)
            (block (result i32)
              (call_indirect (type $check)
                (i32.const 2)
                (block (result i32) (i32.const 1))
                (i32.const 0))))

          (func (export "call_indirect_last") (result i32)
            (block (result i32)
              (call_indirect (type $check)
                (i32.const 1)
                (i32.const 2)
                (block (result i32) (i32.const 0)))))

          (func (export "return_value") (result i32)
            (block (result i32) (i32.const 1))
            (return))

          (func (export "br_value") (result i32)
            (block (result i32)
              (br 0 (block (result i32) (i32.const 1))))))
        "#,
        &store,
        &registry,
    )
    .await;

    for (name, expected) in [
        ("call_indirect_first", 1),
        ("call_indirect_mid", 2),
        ("call_indirect_last", 1),
        ("return_value", 1),
        ("br_value", 1),
    ] {
        let result = run_module_function(&instance, &store, name, &ResultValue::new(vec![])).await;
        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => panic!("{name} block operand case must succeed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn optimizer_specializes_indirect_call_materializers_without_changing_abi() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (type $pick (func (param i32 i32) (result i32)))
          (func $first (type $pick) (param i32 i32) (result i32) (local.get 0))
          (func $second (type $pick) (param i32 i32) (result i32) (local.get 1))
          (func $sum (type $pick) (param i32 i32) (result i32)
            local.get 0
            local.get 1
            i32.add)
          (table funcref (elem $first $second $sum))

          (func (export "call_indirect_local_const_tree") (result i32)
            (local i32)
            (local.set 0 (i32.const 0))
            (call_indirect (type $pick)
              (i32.add (i32.const 40) (i32.const 2))
              (i32.eqz (local.get 0))
              (i32.const 1)))

          (func (export "call_indirect_suffix_partial") (result i32)
            (call_indirect (type $pick)
              (select (i32.const 11) (i32.const 22) (i32.const 0))
              (i32.eqz (i32.const 0))
              (i32.add (i32.const 0) (i32.const 1))))

          (func (export "call_indirect_loop") (result i32)
            (loop (result i32)
              (call_indirect (type $pick)
                (i32.const 9)
                (i32.eqz (i32.const 0))
                (i32.const 1))))

          (func (export "return_call_indirect_eqz") (result i32)
            (return_call_indirect (type $pick)
              (i32.const 11)
              (i32.eqz (i32.const 0))
              (i32.const 1)))

          (func (export "call_indirect_select_tree") (param i32 i32 i32) (result i32)
            (local i32)
            (call_indirect (type $pick)
              (local.get 0)
              (local.get 1)
              (local.get 2)
              select
              (local.get 1)
              (i32.const 1)
              i32.add
              local.tee 3
              i32.eqz
              (i32.const 2)))

          (func (export "return_call_indirect_select_tree") (param i32 i32 i32) (result i32)
            (local i32)
            (return_call_indirect (type $pick)
              (local.get 0)
              (local.get 1)
              (local.get 2)
              select
              (local.get 1)
              (i32.const 1)
              i32.add
              local.tee 3
              i32.eqz
              (i32.const 2))))
        "#,
        &store,
        &registry,
    )
    .await;

    for (name, expected) in [
        ("call_indirect_local_const_tree", 1),
        ("call_indirect_suffix_partial", 1),
        ("call_indirect_loop", 1),
        ("return_call_indirect_eqz", 1),
    ] {
        let result = run_module_function(&instance, &store, name, &ResultValue::new(vec![])).await;
        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => panic!("{name} indirect call relower case must succeed, got {other:?}"),
        }
    }

    for name in [
        "call_indirect_select_tree",
        "return_call_indirect_select_tree",
    ] {
        for ((lhs, rhs, cond), expected) in [
            ((4, -1, 1), 5),
            ((4, 5, 1), 4),
            ((9, -1, 0), 0),
            ((9, 5, 0), 5),
        ] {
            let result = run_module_function(
                &instance,
                &store,
                name,
                &ResultValue::new(vec![
                    WasmValue::I32(lhs),
                    WasmValue::I32(rhs),
                    WasmValue::I32(cond),
                ]),
            )
            .await;

            match result {
                VMResult::Success(values) => {
                    assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
                }
                other => {
                    panic!(
                        "{name} indirect call select tree({lhs}, {rhs}, {cond}) must succeed, got {other:?}"
                    )
                }
            }
        }
    }
}

#[cfg(feature = "simd")]
#[tokio::test]
async fn optimizer_call_relower_const_like_zero_input_leaves_remain_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (type $inspect_t (func (param funcref externref v128) (result i32)))
          (func $inspect (type $inspect_t) (param funcref externref v128) (result i32)
            local.get 0
            ref.is_null
            local.get 1
            ref.is_null
            i32.add
            local.get 2
            i32x4.extract_lane 0
            i32.add)
          (table funcref (elem $inspect))

          (func (export "direct") (result i32)
            (call $inspect
              (ref.func $inspect)
              (ref.null extern)
              (v128.const i32x4 7 0 0 0)))

          (func (export "indirect") (result i32)
            (call_indirect (type $inspect_t)
              (ref.func $inspect)
              (ref.null extern)
              (v128.const i32x4 7 0 0 0)
              (i32.const 0)))

          (func (export "return_indirect") (result i32)
            (return_call_indirect (type $inspect_t)
              (ref.func $inspect)
              (ref.null extern)
              (v128.const i32x4 7 0 0 0)
              (i32.const 0))))
        "#,
        &store,
        &registry,
    )
    .await;

    for name in ["direct", "indirect", "return_indirect"] {
        let result = run_module_function(&instance, &store, name, &ResultValue::new(vec![])).await;
        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(8)]));
            }
            other => panic!("{name} const-like call relower case must succeed, got {other:?}"),
        }
    }

    let mut import_registry = Registry::new();
    let host = instantiate_wat(
        r#"
        (module
          (func (export "inspect") (param funcref externref v128) (result i32)
            local.get 0
            ref.is_null
            local.get 1
            ref.is_null
            i32.add
            local.get 2
            i32x4.extract_lane 0
            i32.add))
        "#,
        &store,
        &import_registry,
    )
    .await;
    import_registry.register("host", host);
    let import_instance = instantiate_wat(
        r#"
        (module
          (import "host" "inspect" (func $inspect (param funcref externref v128) (result i32)))
          (func $dummy)
          (func (export "import_direct") (result i32)
            (call $inspect
              (ref.func $dummy)
              (ref.null extern)
              (v128.const i32x4 7 0 0 0))))
        "#,
        &store,
        &import_registry,
    )
    .await;

    let result = run_module_function(
        &import_instance,
        &store,
        "import_direct",
        &ResultValue::new(vec![]),
    )
    .await;
    match result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(8)]));
        }
        other => panic!("import_direct const-like call relower case must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_call_relower_contiguous_memory_load_leaves_remain_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (type $id_t (func (param i32) (result i32)))
          (func $id (type $id_t) (param i32) (result i32) (local.get 0))
          (table funcref (elem $id))
          (memory 1)

          (func (export "direct_load_arg") (result i32)
            (local $base i32)
            (local.set $base (i32.const 0))
            (i32.store (i32.const 8) (i32.const 37))
            (call $id
              (i32.load
                (i32.add
                  (local.get $base)
                  (i32.const 8)))))

          (func (export "indirect_load_arg") (result i32)
            (local $base i32)
            (local.set $base (i32.const 0))
            (i32.store (i32.const 8) (i32.const 41))
            (call_indirect (type $id_t)
              (i32.load
                (i32.add
                  (local.get $base)
                  (i32.const 8)))
              (i32.const 0)))

          (func (export "return_indirect_load_arg") (result i32)
            (local $base i32)
            (local.set $base (i32.const 0))
            (i32.store (i32.const 8) (i32.const 43))
            (return_call_indirect (type $id_t)
              (i32.load
                (i32.add
                  (local.get $base)
                  (i32.const 8)))
              (i32.const 0)))

          (func (export "indirect_load_index") (result i32)
            (local $base i32)
            (local.set $base (i32.const 0))
            (i32.store (i32.const 4) (i32.const 0))
            (call_indirect (type $id_t)
              (i32.const 21)
              (i32.load
                (i32.add
                  (local.get $base)
                  (i32.const 4))))))
        "#,
        &store,
        &registry,
    )
    .await;

    for (name, expected) in [
        ("direct_load_arg", 37),
        ("indirect_load_arg", 41),
        ("return_indirect_load_arg", 43),
        ("indirect_load_index", 21),
    ] {
        let result = run_module_function(&instance, &store, name, &ResultValue::new(vec![])).await;
        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => panic!("{name} memory-load call relower case must succeed, got {other:?}"),
        }
    }

    let mut import_registry = Registry::new();
    let host = instantiate_wat(
        r#"
        (module
          (func (export "id") (param i32) (result i32) (local.get 0)))
        "#,
        &store,
        &import_registry,
    )
    .await;
    import_registry.register("host", host);
    let import_instance = instantiate_wat(
        r#"
        (module
          (import "host" "id" (func $id (param i32) (result i32)))
          (memory 1)
          (func (export "import_direct_load_arg") (result i32)
            (local $base i32)
            (local.set $base (i32.const 0))
            (i32.store (i32.const 8) (i32.const 55))
            (call $id
              (i32.load
                (i32.add
                  (local.get $base)
                  (i32.const 8))))))
        "#,
        &store,
        &import_registry,
    )
    .await;

    let result = run_module_function(
        &import_instance,
        &store,
        "import_direct_load_arg",
        &ResultValue::new(vec![]),
    )
    .await;
    match result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(55)]));
        }
        other => {
            panic!(
                "import_direct_load_arg memory-load call relower case must succeed, got {other:?}"
            )
        }
    }
}

#[tokio::test]
async fn optimizer_call_relower_nested_call_and_index_trees_remain_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (type $id_t (func (param i32) (result i32)))
          (func $add1 (type $id_t) (param i32) (result i32)
            local.get 0
            i32.const 1
            i32.add)
          (func $id (type $id_t) (param i32) (result i32)
            local.get 0)
          (func $one (result i32) (i32.const 1))
          (table funcref (elem $add1 $id))

          (func (export "direct_nested_direct") (result i32)
            (call $id
              (call $add1 (i32.const 6))))

          (func (export "direct_nested_indirect") (result i32)
            (call $id
              (call_indirect (type $id_t)
                (i32.const 7)
                (i32.const 0))))

          (func (export "indirect_nested_index") (result i32)
            (call_indirect (type $id_t)
              (i32.const 21)
              (call $one)))

          (func (export "return_indirect_nested_arg") (result i32)
            (return_call_indirect (type $id_t)
              (call $add1 (i32.const 10))
              (call $one))))
        "#,
        &store,
        &registry,
    )
    .await;

    for (name, expected) in [
        ("direct_nested_direct", 7),
        ("direct_nested_indirect", 8),
        ("indirect_nested_index", 21),
        ("return_indirect_nested_arg", 11),
    ] {
        let result = run_module_function(&instance, &store, name, &ResultValue::new(vec![])).await;
        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => panic!("{name} nested call relower case must succeed, got {other:?}"),
        }
    }

    let mut import_registry = Registry::new();
    let host = instantiate_wat(
        r#"
        (module
          (func (export "id") (param i32) (result i32)
            local.get 0))
        "#,
        &store,
        &import_registry,
    )
    .await;
    import_registry.register("host", host);
    let import_instance = instantiate_wat(
        r#"
        (module
          (import "host" "id" (func $id (param i32) (result i32)))
          (func $add1 (param i32) (result i32)
            local.get 0
            i32.const 1
            i32.add)
          (func (export "import_nested") (result i32)
            (call $id
              (call $add1 (i32.const 12)))))
        "#,
        &store,
        &import_registry,
    )
    .await;

    let result = run_module_function(
        &import_instance,
        &store,
        "import_nested",
        &ResultValue::new(vec![]),
    )
    .await;
    match result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(13)]));
        }
        other => panic!("import_nested nested call relower case must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn optimizer_call_relower_trap_sensitive_trees_preserve_traps() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func $id32 (param i32) (result i32) (local.get 0))
          (func $id64 (param i64) (result i64) (local.get 0))

          (func (export "div_ok") (result i32)
            (call $id32 (i32.div_s (i32.const 21) (i32.const 3))))

          (func (export "rem_ok") (result i64)
            (call $id64 (i64.rem_u (i64.const 23) (i64.const 5))))

          (func (export "trunc_ok") (result i32)
            (call $id32 (i32.trunc_f32_s (f32.const 7.9))))

          (func (export "div_trap") (result i32)
            (call $id32 (i32.div_s (i32.const 1) (i32.const 0))))

          (func (export "trunc_trap") (result i32)
            (call $id32 (i32.trunc_f32_s (f32.const nan)))))
        "#,
        &store,
        &registry,
    )
    .await;

    for (name, expected) in [
        ("div_ok", ResultValue::new(vec![WasmValue::I32(7)])),
        ("rem_ok", ResultValue::new(vec![WasmValue::I64(3)])),
        ("trunc_ok", ResultValue::new(vec![WasmValue::I32(7)])),
    ] {
        let result = run_module_function(&instance, &store, name, &ResultValue::new(vec![])).await;
        match result {
            VMResult::Success(values) => assert_eq!(values, expected),
            other => panic!("{name} trap-sensitive success case must succeed, got {other:?}"),
        }
    }

    for name in ["div_trap", "trunc_trap"] {
        let result = run_module_function(&instance, &store, name, &ResultValue::new(vec![])).await;
        assert!(result.is_err(), "{name} must preserve its trap");
    }
}

#[tokio::test]
async fn optimizer_call_relower_global_table_select_and_load_trees_remain_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (type $pick_ref (func (param funcref) (result i32)))
          (func $is_non_null (type $pick_ref) (param funcref) (result i32)
            local.get 0
            ref.is_null
            i32.eqz)
          (func $id (param i32) (result i32) (local.get 0))
          (func $add1 (param i32) (result i32)
            local.get 0
            i32.const 1
            i32.add)
          (global $g i32 (i32.const 41))
          (global $base i32 (i32.const 0))
          (table funcref (elem $is_non_null))
          (memory 1)

          (func (export "global_arg") (result i32)
            (call $id (global.get $g)))

          (func (export "table_get_arg") (result i32)
            (call $is_non_null
              (table.get 0 (i32.const 0))))

          (func (export "select_anchored") (param i32) (result i32)
            (call $id
              (select
                (call $add1 (local.get 0))
                (i32.div_s (i32.const 9) (i32.const 3))
                (i32.eqz (local.get 0)))))

          (func (export "load_anchored_address") (result i32)
            (i32.store (i32.const 8) (i32.const 77))
            (call $id
              (i32.load
                (i32.add
                  (global.get $base)
                  (i32.div_u (i32.const 16) (i32.const 2)))))))
        "#,
        &store,
        &registry,
    )
    .await;

    for (name, args, expected) in [
        (
            "global_arg",
            vec![],
            ResultValue::new(vec![WasmValue::I32(41)]),
        ),
        (
            "table_get_arg",
            vec![],
            ResultValue::new(vec![WasmValue::I32(1)]),
        ),
        (
            "select_anchored",
            vec![WasmValue::I32(0)],
            ResultValue::new(vec![WasmValue::I32(1)]),
        ),
        (
            "select_anchored",
            vec![WasmValue::I32(5)],
            ResultValue::new(vec![WasmValue::I32(3)]),
        ),
        (
            "load_anchored_address",
            vec![],
            ResultValue::new(vec![WasmValue::I32(77)]),
        ),
    ] {
        let result = run_module_function(&instance, &store, name, &ResultValue::new(args)).await;
        match result {
            VMResult::Success(values) => assert_eq!(values, expected),
            other => panic!("{name} anchored call relower case must succeed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn optimizer_call_relower_replayed_shared_pure_values_remain_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (type $id_t (func (param i32) (result i32)))
          (func $id (type $id_t) (param i32) (result i32)
            local.get 0)
          (table funcref (elem $id))
          (memory 1)

          (func (export "shared_pure") (param i32) (result i32)
            (drop
              (i32.eqz
                (i32.add
                  (local.get 0)
                  (i32.const 1))))
            (call $id
              (i32.add
                (local.get 0)
                (i32.const 1))))

          (func (export "shared_address_direct") (result i32)
            (local $base i32)
            (local.set $base (i32.const 0))
            (i32.store (i32.const 8) (i32.const 77))
            (drop
              (i32.load
                (i32.add
                  (local.get $base)
                  (i32.const 8))))
            (call $id
              (i32.add
                (local.get $base)
                (i32.const 8))))

          (func (export "shared_address_indirect") (result i32)
            (local $base i32)
            (local.set $base (i32.const 0))
            (i32.store (i32.const 4) (i32.const 0))
            (drop
              (i32.load
                (i32.add
                  (local.get $base)
                  (i32.const 4))))
            (call_indirect (type $id_t)
              (i32.add
                (local.get $base)
                (i32.const 4))
              (i32.const 0))))
        "#,
        &store,
        &registry,
    )
    .await;

    for (name, args, expected) in [
        (
            "shared_pure",
            vec![WasmValue::I32(6)],
            ResultValue::new(vec![WasmValue::I32(7)]),
        ),
        (
            "shared_address_direct",
            vec![],
            ResultValue::new(vec![WasmValue::I32(8)]),
        ),
        (
            "shared_address_indirect",
            vec![],
            ResultValue::new(vec![WasmValue::I32(4)]),
        ),
    ] {
        let result = run_module_function(&instance, &store, name, &ResultValue::new(args)).await;
        match result {
            VMResult::Success(values) => assert_eq!(values, expected),
            other => panic!("{name} replayed call relower case must succeed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn optimizer_call_relower_temp_local_windowing_remains_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (type $inc_t (func (param i32) (result i32)))
          (type $sum3_t (func (param i32 i32 i32) (result i32)))

          (func $inc (type $inc_t) (param i32) (result i32)
            local.get 0
            i32.const 1
            i32.add)

          (func $sum3 (type $sum3_t) (param i32 i32 i32) (result i32)
            local.get 0
            local.get 1
            i32.add
            local.get 2
            i32.add)

          (table 1 funcref)
          (elem (i32.const 0) $sum3)

          (func (export "direct_window") (param i32) (result i32)
            (local $tmp i32)
            (call $sum3
              (i32.const 10)
              (local.tee $tmp
                (call $inc (local.get 0)))
              (block (result i32)
                (drop (i32.eqz (local.get $tmp)))
                (i32.const 2))))

          (func (export "indirect_window") (param i32) (result i32)
            (local $tmp i32)
            (call_indirect (type $sum3_t)
              (i32.const 10)
              (local.tee $tmp
                (call $inc (local.get 0)))
              (block (result i32)
                (drop (i32.eqz (local.get $tmp)))
                (i32.const 2))
              (i32.const 0)))

          (func (export "return_indirect_window") (param i32) (result i32)
            (local $tmp i32)
            (return_call_indirect (type $sum3_t)
              (i32.const 10)
              (local.tee $tmp
                (call $inc (local.get 0)))
              (block (result i32)
                (drop (i32.eqz (local.get $tmp)))
                (i32.const 2))
              (i32.const 0))))
        "#,
        &store,
        &registry,
    )
    .await;

    for name in ["direct_window", "indirect_window", "return_indirect_window"] {
        let result = run_module_function(
            &instance,
            &store,
            name,
            &ResultValue::new(vec![WasmValue::I32(5)]),
        )
        .await;
        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(18)]), "{name}")
            }
            other => panic!("{name} temp-local call windowing case must succeed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn optimizer_call_relower_cross_block_merge_and_return_paths_remain_correct() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (type $id_t (func (param i32) (result i32)))
          (type $inc_t (func (param i32) (result i32)))
          (type $sum3_t (func (param i32 i32 i32) (result i32)))

          (func $id (type $id_t) (param i32) (result i32)
            local.get 0)

          (func $inc (type $inc_t) (param i32) (result i32)
            local.get 0
            i32.const 1
            i32.add)

          (func $sum3 (type $sum3_t) (param i32 i32 i32) (result i32)
            local.get 0
            local.get 1
            i32.add
            local.get 2
            i32.add)

          (table 1 funcref)
          (elem (i32.const 0) $sum3)

          (func (export "merge_direct") (param i32 i32) (result i32)
            (local $tmp i32)
            (call $sum3
              (i32.const 10)
              (local.tee $tmp
                (call $inc (local.get 0)))
              (if (result i32)
                (local.get 1)
                (then
                  (block (result i32)
                    (drop (i32.eqz (local.get $tmp)))
                    (i32.const 2)))
                (else
                  (block (result i32)
                    (drop (i32.eqz (local.get $tmp)))
                    (i32.const 3))))))

          (func (export "merge_indirect") (param i32 i32) (result i32)
            (local $tmp i32)
            (call_indirect (type $sum3_t)
              (i32.const 10)
              (local.tee $tmp
                (call $inc (local.get 0)))
              (if (result i32)
                (local.get 1)
                (then
                  (block (result i32)
                    (drop (i32.eqz (local.get $tmp)))
                    (i32.const 2)))
                (else
                  (block (result i32)
                    (drop (i32.eqz (local.get $tmp)))
                    (i32.const 3))))
              (if (result i32)
                (local.get 1)
                (then (i32.const 0))
                (else (i32.const 0)))))

          (func (export "guarded_return_then_call") (param i32 i32) (result i32)
            (local $tmp i32)
            local.get 1
            if
              local.get 0
              return_call $id
            end
            (call $sum3
              (i32.const 10)
              (local.tee $tmp
                (call $inc (local.get 0)))
              (block (result i32)
                (drop (i32.eqz (local.get $tmp)))
                (i32.const 2))))

          (func (export "guarded_return_then_indirect") (param i32 i32) (result i32)
            (local $tmp i32)
            local.get 1
            if
              local.get 0
              return_call $id
            end
            (return_call_indirect (type $sum3_t)
              (i32.const 10)
              (local.tee $tmp
                (call $inc (local.get 0)))
              (block (result i32)
                (drop (i32.eqz (local.get $tmp)))
                (i32.const 2))
              (if (result i32)
                (local.get 1)
                (then (i32.const 0))
                (else (i32.const 0)))))
        )
        "#,
        &store,
        &registry,
    )
    .await;

    for (name, args, expected) in [
        (
            "merge_direct",
            vec![WasmValue::I32(5), WasmValue::I32(1)],
            ResultValue::new(vec![WasmValue::I32(18)]),
        ),
        (
            "merge_direct",
            vec![WasmValue::I32(5), WasmValue::I32(0)],
            ResultValue::new(vec![WasmValue::I32(19)]),
        ),
        (
            "merge_indirect",
            vec![WasmValue::I32(5), WasmValue::I32(1)],
            ResultValue::new(vec![WasmValue::I32(18)]),
        ),
        (
            "merge_indirect",
            vec![WasmValue::I32(5), WasmValue::I32(0)],
            ResultValue::new(vec![WasmValue::I32(19)]),
        ),
        (
            "guarded_return_then_call",
            vec![WasmValue::I32(5), WasmValue::I32(0)],
            ResultValue::new(vec![WasmValue::I32(18)]),
        ),
        (
            "guarded_return_then_call",
            vec![WasmValue::I32(5), WasmValue::I32(1)],
            ResultValue::new(vec![WasmValue::I32(5)]),
        ),
        (
            "guarded_return_then_indirect",
            vec![WasmValue::I32(5), WasmValue::I32(0)],
            ResultValue::new(vec![WasmValue::I32(18)]),
        ),
        (
            "guarded_return_then_indirect",
            vec![WasmValue::I32(5), WasmValue::I32(1)],
            ResultValue::new(vec![WasmValue::I32(5)]),
        ),
    ] {
        let result = run_module_function(&instance, &store, name, &ResultValue::new(args)).await;
        match result {
            VMResult::Success(values) => assert_eq!(values, expected, "{name}"),
            other => panic!("{name} cross-block call relower case must succeed, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn optimizer_preserves_break_and_block_param_flows() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "break_bare") (result i32)
            (block (br 0) (unreachable))
            (block (br_if 0 (i32.const 1)) (unreachable))
            (block (br_table 0 (i32.const 0)) (unreachable))
            (block (br_table 0 0 0 (i32.const 1)) (unreachable))
            (i32.const 19))

          (func (export "break_value") (result i32)
            (block (result i32)
              (br 0 (i32.const 18))
              (i32.const 19)))

          (func (export "break_multi_value") (result i32 i32 i64)
            (block (result i32 i32 i64)
              (br 0 (i32.const 18) (i32.const -18) (i64.const 18))
              (i32.const 19)
              (i32.const -19)
              (i64.const 19)))

          (func (export "break_repeated") (result i32)
            (block (result i32)
              (br 0 (i32.const 18))
              (br 0 (i32.const 19))
              (drop (br_if 0 (i32.const 20) (i32.const 0)))
              (drop (br_if 0 (i32.const 20) (i32.const 1)))
              (br 0 (i32.const 21))
              (br_table 0 (i32.const 22) (i32.const 4))
              (br_table 0 0 0 (i32.const 23) (i32.const 1))
              (i32.const 21)))

          (func (export "break_inner") (result i32)
            (local i32)
            (local.set 0 (i32.const 0))
            (local.set 0 (i32.add (local.get 0) (block (result i32) (block (result i32) (br 1 (i32.const 0x1))))))
            (local.set 0 (i32.add (local.get 0) (block (result i32) (block (br 0)) (i32.const 0x2))))
            (local.set 0 (i32.add (local.get 0) (block (result i32) (i32.ctz (br 0 (i32.const 0x4))))))
            (local.set 0 (i32.add (local.get 0) (block (result i32) (i32.ctz (block (result i32) (br 1 (i32.const 0x8)))))))
            (local.get 0))

          (func (export "param") (result i32)
            (i32.const 1)
            (block (param i32) (result i32)
              (i32.const 2)
              (i32.add)))

          (func (export "params") (result i32)
            (i32.const 1)
            (i32.const 2)
            (block (param i32 i32) (result i32)
              (i32.add)))

          (func (export "params_id") (result i32)
            (i32.const 1)
            (i32.const 2)
            (block (param i32 i32) (result i32 i32))
            (i32.add))

          (func (export "param_break") (result i32)
            (i32.const 1)
            (block (param i32) (result i32)
              (i32.const 2)
              (i32.add)
              (br 0)))

          (func (export "params_break") (result i32)
            (i32.const 1)
            (i32.const 2)
            (block (param i32 i32) (result i32)
              (i32.add)
              (br 0)))

          (func (export "params_id_break") (result i32)
            (i32.const 1)
            (i32.const 2)
            (block (param i32 i32) (result i32 i32)
              (br 0))
            (i32.add))

          (func (export "effects") (result i32)
            (local i32)
            (block
              (local.set 0 (i32.const 1))
              (local.set 0 (i32.mul (local.get 0) (i32.const 3)))
              (local.set 0 (i32.sub (local.get 0) (i32.const 5)))
              (local.set 0 (i32.mul (local.get 0) (i32.const 7)))
              (br 0)
              (local.set 0 (i32.mul (local.get 0) (i32.const 100))))
            (i32.eq (local.get 0) (i32.const -14))))
        "#,
        &store,
        &registry,
    )
    .await;

    for (name, expected) in [
        ("break_bare", 19),
        ("break_value", 18),
        ("break_repeated", 18),
        ("break_inner", 0x0f),
        ("param", 3),
        ("params", 3),
        ("params_id", 3),
        ("param_break", 3),
        ("params_break", 3),
        ("params_id_break", 3),
        ("effects", 1),
    ] {
        let result = run_module_function(&instance, &store, name, &ResultValue::new(vec![])).await;
        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
            }
            other => panic!("{name} break/param flow case must succeed, got {other:?}"),
        }
    }

    let result = run_module_function(
        &instance,
        &store,
        "break_multi_value",
        &ResultValue::new(vec![]),
    )
    .await;
    match result {
        VMResult::Success(values) => {
            assert_eq!(
                values,
                ResultValue::new(vec![
                    WasmValue::I32(18),
                    WasmValue::I32(-18),
                    WasmValue::I64(18),
                ])
            );
        }
        other => panic!("break_multi_value must succeed, got {other:?}"),
    }
}
