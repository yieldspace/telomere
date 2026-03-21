mod common;
use common::{instantiate_wat, run_wast_with};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
};
use telomere::{
    common::{
        FuncIdx, HostCallContext, HostCallControl, HostTailCallTarget, InstanceHandle, StoreState,
    },
    link_host_function_with_export_name, link_host_function_with_function_idx, Registry,
    ResultValue, Store, VMResult, WasmValue,
};

const PRINT_HOST_WAT: &str = r#"
    (module
      (func (export "print"))
    )
    "#;

const CALL_PRINT_WAST: &str = r#"
    (module
      (import "host" "print" (func $print))
      (func (export "wasm_print") (call $print))
    )
    (invoke "wasm_print")
    "#;

struct LinkState {
    counter: AtomicUsize,
    host: Mutex<Option<InstanceHandle>>,
}

fn host_instance(ctx: &HostCallContext<'_, '_>) -> InstanceHandle {
    let state = ctx.store_state();
    let link_state = unsafe { state.get::<LinkState>() }
        .expect("host function tests require LinkState in StoreState");
    let host = link_state
        .host
        .lock()
        .unwrap()
        .clone()
        .expect("host instance must be recorded before invoking the test host function");
    host
}

fn print(ctx: HostCallContext<'_, '_>) -> VMResult<HostCallControl> {
    let state = ctx.store_state();
    unsafe { state.get::<LinkState>() }
        .expect("host function tests require LinkState in StoreState")
        .counter
        .fetch_add(1, Ordering::SeqCst);
    VMResult::Success(HostCallControl::Return(ResultValue::new(vec![])))
}

fn relink_by_function_idx(ctx: HostCallContext<'_, '_>) -> VMResult<HostCallControl> {
    let host = host_instance(&ctx);
    link_host_function_with_function_idx(&host, 0, print, ctx.store());
    VMResult::Success(HostCallControl::Return(ResultValue::new(vec![])))
}

fn relink_by_export_name(ctx: HostCallContext<'_, '_>) -> VMResult<HostCallControl> {
    let host = host_instance(&ctx);
    link_host_function_with_export_name(&host, "print", print, ctx.store());
    VMResult::Success(HostCallControl::Return(ResultValue::new(vec![])))
}

#[tokio::test]
async fn test_print() {
    let counter = Box::new(LinkState {
        counter: AtomicUsize::new(0),
        host: Mutex::new(None),
    });
    let store = Store::new_with_state(unsafe {
        StoreState::from_ptr(counter.as_ref() as *const LinkState)
    });
    let mut registry = Registry::new();
    let host = instantiate_wat(PRINT_HOST_WAT, &store, &registry).await;
    registry.register("host", host.clone());
    *counter.host.lock().unwrap() = Some(host.clone());
    link_host_function_with_function_idx(&host, 0, print, &store);
    run_wast_with(CALL_PRINT_WAST, &store, &mut registry).await;
    assert_eq!(counter.counter.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_imported_host_start() {
    let counter = Box::new(LinkState {
        counter: AtomicUsize::new(0),
        host: Mutex::new(None),
    });
    let store = Store::new_with_state(unsafe {
        StoreState::from_ptr(counter.as_ref() as *const LinkState)
    });
    let mut registry = Registry::new();
    let host = instantiate_wat(PRINT_HOST_WAT, &store, &registry).await;
    registry.register("host", host.clone());
    *counter.host.lock().unwrap() = Some(host.clone());
    link_host_function_with_function_idx(&host, 0, print, &store);

    let _instance = instantiate_wat(
        r#"
    (module
      (import "host" "print" (func $print))
      (start $print)
    )
    "#,
        &store,
        &registry,
    )
    .await;

    assert_eq!(counter.counter.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_reentrant_link_host_function_with_function_idx_fails_closed() {
    let counter = Box::new(LinkState {
        counter: AtomicUsize::new(0),
        host: Mutex::new(None),
    });
    let store = Store::new_with_state(unsafe {
        StoreState::from_ptr(counter.as_ref() as *const LinkState)
    });
    let mut registry = Registry::new();
    let host = instantiate_wat(PRINT_HOST_WAT, &store, &registry).await;
    registry.register("host", host.clone());
    *counter.host.lock().unwrap() = Some(host.clone());
    link_host_function_with_function_idx(&host, 0, relink_by_function_idx, &store);

    run_wast_with(CALL_PRINT_WAST, &store, &mut registry).await;
    assert_eq!(counter.counter.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn test_reentrant_link_host_function_with_export_name_fails_closed() {
    let counter = Box::new(LinkState {
        counter: AtomicUsize::new(0),
        host: Mutex::new(None),
    });
    let store = Store::new_with_state(unsafe {
        StoreState::from_ptr(counter.as_ref() as *const LinkState)
    });
    let mut registry = Registry::new();
    let host = instantiate_wat(PRINT_HOST_WAT, &store, &registry).await;
    registry.register("host", host.clone());
    *counter.host.lock().unwrap() = Some(host.clone());
    link_host_function_with_function_idx(&host, 0, relink_by_export_name, &store);

    run_wast_with(CALL_PRINT_WAST, &store, &mut registry).await;
    assert_eq!(counter.counter.load(Ordering::SeqCst), 0);
}

fn tail_call(ctx: HostCallContext<'_, '_>) -> VMResult<HostCallControl> {
    let arg = match ctx.param(0) {
        Some(WasmValue::I32(value)) => *value,
        other => panic!("expected i32 param, got {other:?}"),
    };
    VMResult::Success(HostCallControl::TailCall {
        target: HostTailCallTarget::FuncIdx(FuncIdx(1)),
        params: ResultValue::new(vec![WasmValue::I32(arg + 40)]),
    })
}

#[tokio::test]
async fn test_tail_call_wasm() {
    let store = Store::new();
    let mut registry = Registry::new();
    let host = instantiate_wat(
        r#"
    (module
      (func (export "tail_call") (param i32) (result i32) (unreachable))
      (func $plus23 (param i32) (result i32) (i32.add (local.get 0) (i32.const 23)))
    )
    "#,
        &store,
        &registry,
    )
    .await;
    registry.register("host", host.clone());
    link_host_function_with_function_idx(&host, 0, tail_call, &store);
    let wast = r#"
    (module
      (import "host" "tail_call" (func $tail_call (param i32) (result i32)))
      (func (export "tail_call") (param i32) (result i32) (call $tail_call (local.get 0)))
    )
    (assert_return (invoke "tail_call" (i32.const 2)) (i32.const 65))
    "#;
    run_wast_with(wast, &store, &mut registry).await;
}

fn plus60(ctx: HostCallContext<'_, '_>) -> VMResult<HostCallControl> {
    let value = match ctx.param(0) {
        Some(WasmValue::I32(value)) => *value,
        other => panic!("expected i32 param, got {other:?}"),
    };
    VMResult::Success(HostCallControl::Return(ResultValue::new(vec![
        WasmValue::I32(value + 60),
    ])))
}

#[tokio::test]
pub async fn test_tail_call_native() {
    let store = Store::new();
    let mut registry = Registry::new();
    let host = instantiate_wat(
        r#"
    (module
      (func (export "tail_call") (param i32) (result i32) (unreachable))
      (func (param i32) (result i32) (unreachable))
    )
    "#,
        &store,
        &registry,
    )
    .await;
    registry.register("host", host.clone());
    link_host_function_with_function_idx(&host, 0, tail_call, &store);
    link_host_function_with_function_idx(&host, 1, plus60, &store);

    let wast = r#"
    (module
      (import "host" "tail_call" (func $tail_call (param i32) (result i32)))
      (func (export "tail_call") (param i32) (result i32) (call $tail_call (local.get 0)))
    )
    (assert_return (invoke "tail_call" (i32.const 2)) (i32.const 102))
    "#;
    run_wast_with(wast, &store, &mut registry).await;
}
