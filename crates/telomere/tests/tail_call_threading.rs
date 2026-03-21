mod common;

use common::instantiate_wat;
use std::sync::atomic::{AtomicUsize, Ordering};
use telomere::{
    common::{
        AsyncHostCallContext, AsyncHostFunctionDefinition, AsyncHostFuture, AsyncNativeModule,
        FuncType, ValType,
    },
    instantiate_native_async_module, run_module_function, Registry, ResultValue, Store, StoreState,
    VMResult, WasmValue,
};

fn assert_i32_result(result: VMResult<ResultValue>, expected: i32) {
    match result {
        VMResult::Success(values) => match values.iter().next() {
            Some(WasmValue::I32(actual)) => assert_eq!(*actual, expected),
            other => panic!("expected i32 result, got {other:?}"),
        },
        other => panic!("expected success, got {other:?}"),
    }
}

#[tokio::test]
async fn return_call_into_memory_grow_returns_previous_page_count() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1 2)
          (func $grow (param i32) (result i32)
            local.get 0
            memory.grow)
          (func (export "run") (param i32) (result i32)
            local.get 0
            return_call $grow))
        "#,
        &store,
        &registry,
    )
    .await;

    assert_i32_result(
        run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![WasmValue::I32(1)]),
        )
        .await,
        1,
    );
    assert_i32_result(
        run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![WasmValue::I32(1)]),
        )
        .await,
        -1,
    );
}

#[tokio::test]
async fn return_call_into_local_memory_access_roundtrips() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (func $store-load (param i32) (result i32)
            i32.const 0
            local.get 0
            i32.store
            i32.const 0
            i32.load)
          (func (export "run") (param i32) (result i32)
            local.get 0
            return_call $store-load))
        "#,
        &store,
        &registry,
    )
    .await;

    assert_i32_result(
        run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![WasmValue::I32(42)]),
        )
        .await,
        42,
    );
}

#[cfg(feature = "threads")]
#[tokio::test]
async fn return_call_into_shared_memory_access_roundtrips() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1 2 shared)
          (func $store-load (param i32) (result i32)
            i32.const 0
            local.get 0
            i32.store
            i32.const 0
            i32.load)
          (func (export "run") (param i32) (result i32)
            local.get 0
            return_call $store-load))
        "#,
        &store,
        &registry,
    )
    .await;

    assert_i32_result(
        run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![WasmValue::I32(7)]),
        )
        .await,
        7,
    );
}

struct AsyncTailState {
    calls: AtomicUsize,
}

fn async_add_one(ctx: AsyncHostCallContext) -> AsyncHostFuture {
    let state = ctx.store_state;
    let value = match ctx.params.iter().next() {
        Some(WasmValue::I32(value)) => *value,
        other => panic!("expected i32 param, got {other:?}"),
    };
    Box::pin(async move {
        tokio::task::yield_now().await;
        unsafe { state.get::<AsyncTailState>() }
            .unwrap()
            .calls
            .fetch_add(1, Ordering::SeqCst);
        VMResult::Success(ResultValue::new(vec![WasmValue::I32(value + 1)]))
    })
}

#[tokio::test]
async fn return_call_into_async_host_import_resumes_with_result() {
    let state = Box::leak(Box::new(AsyncTailState {
        calls: AtomicUsize::new(0),
    }));
    let store = Store::new_with_state(StoreState::from_static(state));
    let mut registry = Registry::new();
    let host = instantiate_native_async_module(
        AsyncNativeModule {
            functions: vec![AsyncHostFunctionDefinition {
                name: Some("add-one".to_owned()),
                signature: FuncType::new(vec![ValType::I32], vec![ValType::I32]),
                fp: async_add_one,
            }],
        },
        &store,
        &registry,
    )
    .await
    .unwrap();
    registry.register("host", host);

    let instance = instantiate_wat(
        r#"
        (module
          (import "host" "add-one" (func $add-one (param i32) (result i32)))
          (func (export "run") (param i32) (result i32)
            local.get 0
            return_call $add-one))
        "#,
        &store,
        &registry,
    )
    .await;

    assert_i32_result(
        run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![WasmValue::I32(41)]),
        )
        .await,
        42,
    );
    assert_eq!(state.calls.load(Ordering::SeqCst), 1);
}
