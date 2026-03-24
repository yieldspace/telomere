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
