mod common;

use common::{instantiate_wat, run_wast_with};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
};
use telomere::{
    common::{AsyncHostFunctionDefinition, AsyncHostFuture, AsyncNativeModule, FuncType, ValType},
    get_global, instantiate_native_async_module, link_async_host_function_with_export_name,
    link_async_host_function_with_function_idx, Registry, ResultValue, Store, StoreState, VMResult,
    WasmValue,
};

struct ScalarState {
    calls: AtomicUsize,
}

fn async_add_one(state: StoreState, args: ResultValue) -> AsyncHostFuture {
    Box::pin(async move {
        tokio::task::yield_now().await;
        let state = unsafe { state.get::<ScalarState>() }.unwrap();
        state.calls.fetch_add(1, Ordering::SeqCst);
        let value = match args.iter().collect::<Vec<_>>().as_slice() {
            [WasmValue::I32(value)] => *value,
            _ => return VMResult::InvalidOperand,
        };
        VMResult::Success(ResultValue::new(vec![WasmValue::I32(value + 1)]))
    })
}

struct RoundTripState {
    calls: AtomicUsize,
    seen: Mutex<Vec<(i32, i64)>>,
}

fn async_swap_results(state: StoreState, args: ResultValue) -> AsyncHostFuture {
    Box::pin(async move {
        tokio::task::yield_now().await;
        let state = unsafe { state.get::<RoundTripState>() }.unwrap();
        let (lhs, rhs) = match args.iter().collect::<Vec<_>>().as_slice() {
            [WasmValue::I32(lhs), WasmValue::I64(rhs)] => (*lhs, *rhs),
            _ => return VMResult::InvalidOperand,
        };
        state.calls.fetch_add(1, Ordering::SeqCst);
        state.seen.lock().unwrap().push((lhs, rhs));
        VMResult::Success(ResultValue::new(vec![
            WasmValue::I64(rhs),
            WasmValue::I32(lhs),
        ]))
    })
}

struct StartState {
    calls: AtomicUsize,
}

fn async_init(state: StoreState, args: ResultValue) -> AsyncHostFuture {
    Box::pin(async move {
        tokio::task::yield_now().await;
        if args.len() != 0 {
            return VMResult::InvalidOperand;
        }
        unsafe { state.get::<StartState>() }
            .unwrap()
            .calls
            .fetch_add(1, Ordering::SeqCst);
        VMResult::Success(ResultValue::new(vec![]))
    })
}

struct CallIndirectState {
    calls: AtomicUsize,
}

fn async_double(state: StoreState, args: ResultValue) -> AsyncHostFuture {
    Box::pin(async move {
        tokio::task::yield_now().await;
        let state = unsafe { state.get::<CallIndirectState>() }.unwrap();
        state.calls.fetch_add(1, Ordering::SeqCst);
        let value = match args.iter().collect::<Vec<_>>().as_slice() {
            [WasmValue::I32(value)] => *value,
            _ => return VMResult::InvalidOperand,
        };
        VMResult::Success(ResultValue::new(vec![WasmValue::I32(value * 2)]))
    })
}

fn async_fail(_state: StoreState, _args: ResultValue) -> AsyncHostFuture {
    Box::pin(async move {
        tokio::task::yield_now().await;
        VMResult::InvalidOperand
    })
}

#[tokio::test]
async fn async_import_returns_scalar_after_yield() {
    let state = Box::leak(Box::new(ScalarState {
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
    run_wast_with(
        r#"
        (module
          (import "host" "add-one" (func $add-one (param i32) (result i32)))
          (func (export "run") (param i32) (result i32)
            local.get 0
            call $add-one))
        (assert_return (invoke "run" (i32.const 41)) (i32.const 42))
        "#,
        &store,
        &mut registry,
    )
    .await;
    assert_eq!(state.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn sync_run_rejects_async_host_imports() {
    let store = Store::new();
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
            call $add-one))
        "#,
        &store,
        &registry,
    )
    .await;
    let mut gc = store.lock_gc();
    let result = telomere::component_support::runtime::run_module_function_sync_with_gc(
        &instance,
        &store,
        &mut gc,
        "run",
        &ResultValue::new(vec![WasmValue::I32(41)]),
    );
    assert!(matches!(result, Err(error) if error == "AsyncPending"));
}

#[tokio::test]
async fn async_import_roundtrips_multi_value_params_and_results() {
    let state = Box::leak(Box::new(RoundTripState {
        calls: AtomicUsize::new(0),
        seen: Mutex::new(vec![]),
    }));
    let store = Store::new_with_state(StoreState::from_static(state));
    let mut registry = Registry::new();
    let host = instantiate_native_async_module(
        AsyncNativeModule {
            functions: vec![AsyncHostFunctionDefinition {
                name: Some("swap".to_owned()),
                signature: FuncType::new(
                    vec![ValType::I32, ValType::I64],
                    vec![ValType::I64, ValType::I32],
                ),
                fp: async_swap_results,
            }],
        },
        &store,
        &registry,
    )
    .await
    .unwrap();
    registry.register("host", host);
    run_wast_with(
        r#"
        (module
          (import "host" "swap" (func $swap (param i32 i64) (result i64 i32)))
          (func (export "run") (param i32 i64) (result i64 i32)
            local.get 0
            local.get 1
            call $swap))
        (assert_return
          (invoke "run" (i32.const 7) (i64.const 9))
          (i64.const 9)
          (i32.const 7))
        "#,
        &store,
        &mut registry,
    )
    .await;
    assert_eq!(state.calls.load(Ordering::SeqCst), 1);
    assert_eq!(state.seen.lock().unwrap().as_slice(), &[(7, 9)]);
}

#[tokio::test]
async fn async_import_can_run_from_start_function() {
    let state = Box::leak(Box::new(StartState {
        calls: AtomicUsize::new(0),
    }));
    let store = Store::new_with_state(StoreState::from_static(state));
    let registry = Registry::new();
    let host = instantiate_native_async_module(
        AsyncNativeModule {
            functions: vec![AsyncHostFunctionDefinition {
                name: Some("init".to_owned()),
                signature: FuncType::new(vec![], vec![]),
                fp: async_init,
            }],
        },
        &store,
        &registry,
    )
    .await
    .unwrap();
    let mut registry = registry;
    registry.register("host", host);
    let instance = instantiate_wat(
        r#"
        (module
          (import "host" "init" (func $init))
          (global (export "ready") i32 (i32.const 7))
          (start $init))
        "#,
        &store,
        &registry,
    )
    .await;
    assert_eq!(state.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        get_global(&instance, &store, "ready").unwrap(),
        WasmValue::I32(7)
    );
}

#[tokio::test]
async fn async_import_supports_call_indirect_after_linking() {
    let state = Box::leak(Box::new(CallIndirectState {
        calls: AtomicUsize::new(0),
    }));
    let store = Store::new_with_state(StoreState::from_static(state));
    let mut registry = Registry::new();
    let host = instantiate_wat(
        r#"
        (module
          (func (export "double") (param i32) (result i32)
            unreachable))
        "#,
        &store,
        &registry,
    )
    .await;
    link_async_host_function_with_export_name(&host, "double", async_double, &store);
    registry.register("host", host);
    run_wast_with(
        r#"
        (module
          (type $t (func (param i32) (result i32)))
          (import "host" "double" (func $double (type $t)))
          (table 1 funcref)
          (elem (i32.const 0) func $double)
          (func (export "run") (param i32) (result i32)
            local.get 0
            i32.const 0
            call_indirect (type $t)))
        (assert_return (invoke "run" (i32.const 21)) (i32.const 42))
        "#,
        &store,
        &mut registry,
    )
    .await;
    assert_eq!(state.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn async_import_error_propagates_to_caller() {
    let store = Store::new();
    let mut registry = Registry::new();
    let host = instantiate_wat(
        r#"
        (module
          (func (export "fail")))
        "#,
        &store,
        &registry,
    )
    .await;
    link_async_host_function_with_function_idx(&host, 0, async_fail, &store);
    registry.register("host", host);
    let instance = instantiate_wat(
        r#"
        (module
          (import "host" "fail" (func $fail))
          (func (export "run")
            call $fail))
        "#,
        &store,
        &registry,
    )
    .await;
    let result =
        telomere::run_module_function(&instance, &store, "run", &ResultValue::new(vec![]))
            .await;
    assert!(matches!(result, VMResult::InvalidOperand));
}
