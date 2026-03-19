use common::instantiate_wat;
use telomere::{run_module_function, Registry, ResultValue, Store, WasmValue};

mod common;

#[tokio::test]
async fn effect_bulk() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (func (export "test") (result i32)
            i32.const 0
            i32.const 1
            i32.store
            i32.const 1
            i32.const 2
            i32.store8
            i32.const 2
            i32.const 0
            i32.const 2
            memory.copy
            i32.const 3
            i32.load8_u))
        "#,
        &store,
        &registry,
    )
    .await;

    let result = run_module_function(&instance, &store, "test", &ResultValue::new(vec![]))
        .await
        .unwrap();
    assert_eq!(result.iter().collect::<Vec<_>>(), vec![&WasmValue::I32(2)]);
}

#[tokio::test]
async fn function_can_complete_after_terminal_memory_write_effect() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory (export "memory") 1)
          (func (export "store_then_return") (param i32)
            i32.const 0
            local.get 0
            i32.store)
          (func (export "read_back") (result i32)
            i32.const 0
            i32.load))
        "#,
        &store,
        &registry,
    )
    .await;

    run_module_function(
        &instance,
        &store,
        "store_then_return",
        &ResultValue::new(vec![WasmValue::I32(123)]),
    )
    .await
    .unwrap();

    let result = run_module_function(&instance, &store, "read_back", &ResultValue::new(vec![]))
        .await
        .unwrap();
    let values = result.iter().collect::<Vec<_>>();
    assert_eq!(values, vec![&WasmValue::I32(123)]);
}

#[tokio::test]
async fn shift_counts_follow_wasm_modulo_semantics() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "i32_shl_32") (result i32)
            i32.const 1
            i32.const 32
            i32.shl)
          (func (export "i32_shr_u_32") (result i32)
            i32.const -1
            i32.const 32
            i32.shr_u)
          (func (export "i64_shl_64") (result i64)
            i64.const 1
            i64.const 64
            i64.shl)
          (func (export "i64_shr_u_64") (result i64)
            i64.const -1
            i64.const 64
            i64.shr_u))
        "#,
        &store,
        &registry,
    )
    .await;

    let i32_shl = run_module_function(&instance, &store, "i32_shl_32", &ResultValue::new(vec![]))
        .await
        .unwrap();
    assert_eq!(i32_shl.iter().collect::<Vec<_>>(), vec![&WasmValue::I32(1)]);

    let i32_shr_u =
        run_module_function(&instance, &store, "i32_shr_u_32", &ResultValue::new(vec![]))
            .await
            .unwrap();
    assert_eq!(
        i32_shr_u.iter().collect::<Vec<_>>(),
        vec![&WasmValue::I32(-1)]
    );

    let i64_shl = run_module_function(&instance, &store, "i64_shl_64", &ResultValue::new(vec![]))
        .await
        .unwrap();
    assert_eq!(i64_shl.iter().collect::<Vec<_>>(), vec![&WasmValue::I64(1)]);

    let i64_shr_u =
        run_module_function(&instance, &store, "i64_shr_u_64", &ResultValue::new(vec![]))
            .await
            .unwrap();
    assert_eq!(
        i64_shr_u.iter().collect::<Vec<_>>(),
        vec![&WasmValue::I64(-1)]
    );
}

#[tokio::test]
async fn wasm_locals_are_zero_initialized() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "read_local") (result i32)
            (local i32)
            local.get 0)
          (func (export "call_read_local") (result i32)
            call 0))
        "#,
        &store,
        &registry,
    )
    .await;

    let direct = run_module_function(&instance, &store, "read_local", &ResultValue::new(vec![]))
        .await
        .unwrap();
    assert_eq!(direct.iter().collect::<Vec<_>>(), vec![&WasmValue::I32(0)]);

    let nested = run_module_function(
        &instance,
        &store,
        "call_read_local",
        &ResultValue::new(vec![]),
    )
    .await
    .unwrap();
    assert_eq!(nested.iter().collect::<Vec<_>>(), vec![&WasmValue::I32(0)]);
}
