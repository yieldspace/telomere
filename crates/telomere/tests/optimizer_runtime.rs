mod common;

use common::instantiate_wat;
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
