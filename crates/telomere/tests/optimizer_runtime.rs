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
