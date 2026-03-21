mod common;
use common::run_wast_with;
use std::sync::atomic::{AtomicUsize, Ordering};
use telomere::{
    common::{
        FuncType, GcRef, HostCallContext, HostCallControl, HostFunctionDefinition,
        HostTailCallTarget, NativeModule, StoreState, ValType,
    },
    link_host_function_with_function_idx,
    runtime::instantiate_native_module,
    Registry, ResultValue, Store, VMResult, WasmValue,
};

fn print(ctx: HostCallContext<'_, '_>) -> VMResult<HostCallControl> {
    let state = ctx.store_state();
    unsafe { state.get::<AtomicUsize>() }
        .expect("host function tests require an AtomicUsize in StoreState")
        .fetch_add(1, Ordering::SeqCst);
    VMResult::Success(HostCallControl::Return(ResultValue::new(vec![])))
}

#[tokio::test]
async fn test_print() {
    let counter = Box::new(AtomicUsize::new(0));
    let store = Store::new_with_state(unsafe {
        StoreState::from_ptr(counter.as_ref() as *const AtomicUsize)
    });
    let mut registry = Registry::new();
    let host = instantiate_native_module(
        NativeModule {
            functions: vec![HostFunctionDefinition {
                fp: print,
                name: Some("print".to_string()),
                signature: FuncType::new(vec![], vec![]),
            }],
        },
        &store,
        &registry,
    )
    .await
    .unwrap();
    registry.register("host", host.clone());
    link_host_function_with_function_idx(&host, 0, print, &store);
    let wast = r#"
    (module
      (import "host" "print" (func $print))
      (func (export "wasm_print") (call $print))
    )
    (invoke "wasm_print")
    "#;
    run_wast_with(wast, &store, &mut registry).await;
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

fn tail_call(ctx: HostCallContext<'_, '_>) -> VMResult<HostCallControl> {
    let func_ref = match ctx.param(0) {
        Some(WasmValue::FuncRef(value)) => *value,
        other => panic!("expected funcref param, got {other:?}"),
    };
    let arg = match ctx.param(1) {
        Some(WasmValue::I32(value)) => *value,
        other => panic!("expected i32 param, got {other:?}"),
    };
    VMResult::Success(HostCallControl::TailCall {
        target: HostTailCallTarget::FuncRef(GcRef(func_ref)),
        params: ResultValue::new(vec![WasmValue::I32(arg + 40)]),
    })
}

#[tokio::test]
async fn test_tail_call_wasm() {
    let store = Store::new();
    let mut registry = Registry::new();
    let host = instantiate_native_module(
        NativeModule {
            functions: vec![HostFunctionDefinition {
                fp: tail_call,
                name: Some("tail_call".to_string()),
                signature: FuncType::new(vec![ValType::FuncRef, ValType::I32], vec![ValType::I32]),
            }],
        },
        &store,
        &registry,
    )
    .await
    .unwrap();
    registry.register("host", host.clone());
    link_host_function_with_function_idx(&host, 0, tail_call, &store);
    let wast = r#"
    (module
      (import "host" "tail_call" (func $tail_call (param funcref i32) (result i32)))
      (func $plus23 (param i32) (result i32) (i32.add (local.get 0) (i32.const 23)))
      (func (export "tail_call") (param i32) (result i32) (call $tail_call (ref.func $plus23) (local.get 0)))
      (elem declare func $plus23)
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
    let host = instantiate_native_module(
        NativeModule {
            functions: vec![
                HostFunctionDefinition {
                    fp: tail_call,
                    name: Some("tail_call".to_string()),
                    signature: FuncType::new(
                        vec![ValType::FuncRef, ValType::I32],
                        vec![ValType::I32],
                    ),
                },
                HostFunctionDefinition {
                    fp: plus60,
                    name: Some("plus60".to_string()),
                    signature: FuncType::new(vec![ValType::I32], vec![ValType::I32]),
                },
            ],
        },
        &store,
        &registry,
    )
    .await
    .unwrap();
    registry.register("host", host.clone());
    link_host_function_with_function_idx(&host, 0, tail_call, &store);
    link_host_function_with_function_idx(&host, 1, plus60, &store);

    let wast = r#"
    (module
      (import "host" "tail_call" (func $tail_call (param funcref i32) (result i32)))
      (import "host" "plus60" (func $plus60 (param i32) (result i32)))
      (func (export "tail_call") (param i32) (result i32) (call $tail_call (ref.func $plus60) (local.get 0)))
      (elem declare func $plus60)
    )
    (assert_return (invoke "tail_call" (i32.const 2)) (i32.const 102))
    "#;
    run_wast_with(wast, &store, &mut registry).await;
}
