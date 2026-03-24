mod common;

use common::{instantiate_wat, run_wast_with};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
};
use std::{collections::VecDeque, future::Future, pin::Pin};
use telomere::{
    common::{
        AsyncHostFunctionDefinition, AsyncHostFuture, AsyncNativeModule, ExecuteContext, FuncType,
        ValType,
    },
    get_global, instantiate_native_async_module, link_async_host_function_with_export_name,
    link_async_host_function_with_function_idx, link_host_function_with_function_idx, Completion,
    CompletionPayload, ExecutionDriver, MemoryWaitPending, PendingOp, Registry, ResultValue, Store,
    StoreState, VMResult, WasmValue,
};

struct MockDriver {
    submitted: usize,
    inflight: VecDeque<Pin<Box<dyn Future<Output = Completion>>>>,
}

impl MockDriver {
    fn new() -> Self {
        Self {
            submitted: 0,
            inflight: VecDeque::new(),
        }
    }
}

impl ExecutionDriver for MockDriver {
    fn submit(&mut self, op: PendingOp) {
        self.submitted += 1;
        match op {
            PendingOp::HostCall(op) => {
                self.inflight.push_back(Box::pin(op.into_completion()));
            }
            PendingOp::MemoryWait(MemoryWaitPending {
                task_id,
                shared,
                wait,
                timeout_ns,
                fp,
            }) => {
                self.inflight.push_back(Box::pin(async move {
                    let value = wait.wait_result(shared, timeout_ns).await;
                    Completion {
                        task_id,
                        payload: CompletionPayload::ResumeWithI32 { fp, value },
                    }
                }));
            }
            PendingOp::WasmAsync(op) => {
                panic!(
                    "unexpected wasm async pending op for task {} in mock driver",
                    op.task_id
                );
            }
        }
    }

    fn next_completion<'a>(&'a mut self) -> Pin<Box<dyn Future<Output = Option<Completion>> + 'a>> {
        Box::pin(async move {
            let future = self.inflight.pop_front()?;
            Some(future.await)
        })
    }
}

struct ScalarState {
    calls: AtomicUsize,
}

fn async_add_one(ctx: &mut ExecuteContext<'_>) -> AsyncHostFuture {
    let state = ctx.store.state;
    let value = i32::from_le_bytes(
        ctx.stack
            .local_bytes(&ctx.local_reference(), 0, 4)
            .try_into()
            .unwrap(),
    );
    let slot = ctx.return_slot();
    let (prev_local_ref, return_addr) =
        ctx.stack
            .function_return_in_place(&ctx.local_reference, 4, ctx.gc);
    ctx.set_local_reference(prev_local_ref);
    Box::pin(async move {
        tokio::task::yield_now().await;
        let state = unsafe { state.get::<ScalarState>() }.unwrap();
        state.calls.fetch_add(1, Ordering::SeqCst);
        slot.write(&(value + 1).to_le_bytes());
        VMResult::Success(return_addr)
    })
}

struct RoundTripState {
    calls: AtomicUsize,
    seen: Mutex<Vec<(i32, i64)>>,
}

fn async_swap_results(ctx: &mut ExecuteContext<'_>) -> AsyncHostFuture {
    let state = ctx.store.state;
    let local_ref = ctx.local_reference();
    let lhs = i32::from_le_bytes(ctx.stack.local_bytes(&local_ref, 0, 4).try_into().unwrap());
    let rhs = i64::from_le_bytes(ctx.stack.local_bytes(&local_ref, 4, 8).try_into().unwrap());
    let slot = ctx.return_slot();
    let (prev_local_ref, return_addr) =
        ctx.stack
            .function_return_in_place(&ctx.local_reference, 12, ctx.gc);
    ctx.set_local_reference(prev_local_ref);
    Box::pin(async move {
        tokio::task::yield_now().await;
        let state = unsafe { state.get::<RoundTripState>() }.unwrap();
        state.calls.fetch_add(1, Ordering::SeqCst);
        state.seen.lock().unwrap().push((lhs, rhs));
        let mut result = [0u8; 12];
        result[0..8].copy_from_slice(&rhs.to_le_bytes());
        result[8..12].copy_from_slice(&lhs.to_le_bytes());
        slot.write(&result);
        VMResult::Success(return_addr)
    })
}

struct StartState {
    calls: AtomicUsize,
}

fn async_init(ctx: &mut ExecuteContext<'_>) -> AsyncHostFuture {
    let state = ctx.store.state;
    let (prev_local_ref, return_addr) =
        ctx.stack
            .function_return_in_place(&ctx.local_reference, 0, ctx.gc);
    ctx.set_local_reference(prev_local_ref);
    Box::pin(async move {
        tokio::task::yield_now().await;
        unsafe { state.get::<StartState>() }
            .unwrap()
            .calls
            .fetch_add(1, Ordering::SeqCst);
        VMResult::Success(return_addr)
    })
}

struct CallIndirectState {
    calls: AtomicUsize,
}

fn async_double(ctx: &mut ExecuteContext<'_>) -> AsyncHostFuture {
    let state = ctx.store.state;
    let value = i32::from_le_bytes(
        ctx.stack
            .local_bytes(&ctx.local_reference(), 0, 4)
            .try_into()
            .unwrap(),
    );
    let slot = ctx.return_slot();
    let (prev_local_ref, return_addr) =
        ctx.stack
            .function_return_in_place(&ctx.local_reference, 4, ctx.gc);
    ctx.set_local_reference(prev_local_ref);
    Box::pin(async move {
        tokio::task::yield_now().await;
        let state = unsafe { state.get::<CallIndirectState>() }.unwrap();
        state.calls.fetch_add(1, Ordering::SeqCst);
        slot.write(&(value * 2).to_le_bytes());
        VMResult::Success(return_addr)
    })
}

fn async_fail(ctx: &mut ExecuteContext<'_>) -> AsyncHostFuture {
    let (_prev_local_ref, _return_addr) =
        ctx.stack
            .function_return_in_place(&ctx.local_reference, 0, ctx.gc);
    ctx.set_local_reference(_prev_local_ref);
    Box::pin(async move {
        tokio::task::yield_now().await;
        VMResult::InvalidOperand
    })
}

fn sync_add_two(ctx: &mut ExecuteContext) -> VMResult<*const telomere::common::Instr> {
    let value = i32::from_le_bytes(
        ctx.stack
            .local_bytes(&ctx.local_reference(), 0, 4)
            .try_into()
            .unwrap(),
    );
    let slot = ctx.return_slot();
    slot.write(&(value + 2).to_le_bytes());
    let (prev_local_ref, return_addr) =
        ctx.stack
            .function_return_in_place(&ctx.local_reference, 4, ctx.gc);
    ctx.set_local_reference(prev_local_ref);
    VMResult::Success(return_addr)
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
    let result = telomere::component_support::runtime::run_core_export_sync_reentrant(
        &instance,
        &store,
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
async fn host_link_can_switch_from_async_to_sync() {
    let state = Box::leak(Box::new(ScalarState {
        calls: AtomicUsize::new(0),
    }));
    let store = Store::new_with_state(StoreState::from_static(state));
    let mut registry = Registry::new();
    let host = instantiate_wat(
        r#"
        (module
          (func (export "flip") (param i32) (result i32)
            unreachable))
        "#,
        &store,
        &registry,
    )
    .await;
    link_async_host_function_with_export_name(&host, "flip", async_add_one, &store);
    registry.register("host", host.clone());
    let instance = instantiate_wat(
        r#"
        (module
          (import "host" "flip" (func $flip (param i32) (result i32)))
          (func (export "run") (param i32) (result i32)
            local.get 0
            call $flip))
        "#,
        &store,
        &registry,
    )
    .await;
    let first = telomere::run_module_function(
        &instance,
        &store,
        "run",
        &ResultValue::new(vec![WasmValue::I32(40)]),
    )
    .await;
    assert!(matches!(
        first,
        VMResult::Success(ref result) if result == &ResultValue::new(vec![WasmValue::I32(41)])
    ));
    link_host_function_with_function_idx(&host, 0, sync_add_two, &store);
    let second = telomere::run_module_function(
        &instance,
        &store,
        "run",
        &ResultValue::new(vec![WasmValue::I32(40)]),
    )
    .await;
    assert!(matches!(
        second,
        VMResult::Success(ref result) if result == &ResultValue::new(vec![WasmValue::I32(42)])
    ));
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
        telomere::run_module_function(&instance, &store, "run", &ResultValue::new(vec![])).await;
    assert!(matches!(result, VMResult::InvalidOperand));
}

#[tokio::test]
async fn async_import_can_run_with_custom_driver() {
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

    let mut driver = MockDriver::new();
    let result = telomere::run_module_function_with_driver(
        &instance,
        &store,
        "run",
        &ResultValue::new(vec![WasmValue::I32(41)]),
        &mut driver,
    )
    .await;

    assert!(matches!(
        result,
        VMResult::Success(ref values) if values == &ResultValue::new(vec![WasmValue::I32(42)])
    ));
    assert_eq!(driver.submitted, 1);
    assert_eq!(state.calls.load(Ordering::SeqCst), 1);
}
