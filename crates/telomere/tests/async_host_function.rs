mod common;

use common::{instantiate_wat, run_wast_with};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
};
use std::{collections::VecDeque, future::Future, pin::Pin};
#[cfg(feature = "threads")]
use telomere::MemoryWaitPending;
use telomere::{
    component_support::common::{FuncType, ValType},
    get_global, instantiate_native_async_module, instantiate_native_module,
    link_async_host_function_with_export_name, link_async_host_function_with_function_idx,
    link_host_function_with_function_idx, AsyncHostFnDefinition, AsyncHostModule, Completion,
    CompletionPayload, ExecutionDriver, HostCallPending, HostContext, HostFnDefinition, HostFuture,
    HostModule, PendingOp, Registry, ResultValue, Store, StoreState, TokioDriver, VMResult,
    WasmValue,
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
            PendingOp::HostCall(HostCallPending {
                task_id, future, ..
            }) => {
                self.inflight.push_back(Box::pin(async move {
                    Completion {
                        task_id,
                        payload: CompletionPayload::HostCall {
                            result: future.await,
                        },
                    }
                }));
            }
            #[cfg(feature = "threads")]
            PendingOp::MemoryWait(MemoryWaitPending {
                task_id,
                shared,
                wait,
                timeout_ns,
                fp,
                interrupt,
            }) => {
                self.inflight.push_back(Box::pin(async move {
                    let payload = match wait
                        .wait_result_interruptible(shared, timeout_ns, interrupt)
                        .await
                    {
                        VMResult::Success(value) => CompletionPayload::ResumeWithI32 { fp, value },
                        VMResult::Cancelled => CompletionPayload::Interrupted,
                        _ => unreachable!("memory wait resolves only with a code or cancellation"),
                    };
                    Completion { task_id, payload }
                }));
            }
            #[cfg(feature = "unstable-internals")]
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

fn async_add_one(cx: &mut HostContext<'_, '_>) -> HostFuture {
    let state = cx.store().state;
    let WasmValue::I32(value) = cx.args()[0] else {
        panic!("async_add_one expects an i32 argument");
    };
    Box::pin(async move {
        tokio::task::yield_now().await;
        let state = unsafe { state.get::<ScalarState>() }.unwrap();
        state.calls.fetch_add(1, Ordering::SeqCst);
        VMResult::Success(vec![WasmValue::I32(value + 1)])
    })
}

struct RoundTripState {
    calls: AtomicUsize,
    seen: Mutex<Vec<(i32, i64)>>,
}

fn async_swap_results(cx: &mut HostContext<'_, '_>) -> HostFuture {
    let state = cx.store().state;
    let (WasmValue::I32(lhs), WasmValue::I64(rhs)) = (cx.args()[0], cx.args()[1]) else {
        panic!("async_swap_results expects (i32, i64) arguments");
    };
    Box::pin(async move {
        tokio::task::yield_now().await;
        let state = unsafe { state.get::<RoundTripState>() }.unwrap();
        state.calls.fetch_add(1, Ordering::SeqCst);
        state.seen.lock().unwrap().push((lhs, rhs));
        VMResult::Success(vec![WasmValue::I64(rhs), WasmValue::I32(lhs)])
    })
}

struct StartState {
    calls: AtomicUsize,
}

fn async_init(cx: &mut HostContext<'_, '_>) -> HostFuture {
    let state = cx.store().state;
    Box::pin(async move {
        tokio::task::yield_now().await;
        unsafe { state.get::<StartState>() }
            .unwrap()
            .calls
            .fetch_add(1, Ordering::SeqCst);
        VMResult::Success(vec![])
    })
}

struct CallIndirectState {
    calls: AtomicUsize,
}

fn async_double(cx: &mut HostContext<'_, '_>) -> HostFuture {
    let state = cx.store().state;
    let WasmValue::I32(value) = cx.args()[0] else {
        panic!("async_double expects an i32 argument");
    };
    Box::pin(async move {
        tokio::task::yield_now().await;
        let state = unsafe { state.get::<CallIndirectState>() }.unwrap();
        state.calls.fetch_add(1, Ordering::SeqCst);
        VMResult::Success(vec![WasmValue::I32(value * 2)])
    })
}

fn async_fail(_cx: &mut HostContext<'_, '_>) -> HostFuture {
    Box::pin(async move {
        tokio::task::yield_now().await;
        VMResult::InvalidOperand
    })
}

fn sync_add_two(cx: &mut HostContext<'_, '_>) -> VMResult<Vec<WasmValue>> {
    let WasmValue::I32(value) = cx.args()[0] else {
        return VMResult::Unreachable;
    };
    VMResult::Success(vec![WasmValue::I32(value + 2)])
}

#[tokio::test]
async fn async_import_returns_scalar_after_yield() {
    let state = Box::leak(Box::new(ScalarState {
        calls: AtomicUsize::new(0),
    }));
    let store = Store::new_with_state(StoreState::from_static(state));
    let mut registry = Registry::new();
    let host = instantiate_native_async_module(
        AsyncHostModule {
            functions: vec![AsyncHostFnDefinition {
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
        AsyncHostModule {
            functions: vec![AsyncHostFnDefinition {
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
        AsyncHostModule {
            functions: vec![AsyncHostFnDefinition {
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
        AsyncHostModule {
            functions: vec![AsyncHostFnDefinition {
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
        AsyncHostModule {
            functions: vec![AsyncHostFnDefinition {
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

/// Regression test for the #216 review (use-after-free): a submitted
/// host-call future is owned by the embedder's driver and may still be
/// polled after the execution that submitted it has been dropped, so the
/// future must never write into the task's stack. Cancelling the run frees
/// the scheduler and its stacks while the `TokioDriver` retains the
/// submitted future in its in-flight set; polling it to completion
/// afterwards must be harmless, and the same driver must still drive a
/// fresh run to the correct result (the scheduler applies the values when
/// it applies the completion).
#[tokio::test]
async fn dropped_execution_leaves_retained_driver_future_harmless() {
    let state = Box::leak(Box::new(ScalarState {
        calls: AtomicUsize::new(0),
    }));
    let store = Store::new_with_state(StoreState::from_static(state));
    let mut registry = Registry::new();
    let host = instantiate_native_async_module(
        AsyncHostModule {
            functions: vec![AsyncHostFnDefinition {
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

    let mut driver = TokioDriver::new();
    {
        let args = ResultValue::new(vec![WasmValue::I32(41)]);
        let run =
            telomere::run_module_function_with_driver(&instance, &store, "run", &args, &mut driver);
        tokio::pin!(run);
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(waker);
        // One poll runs the guest until the host call has been submitted to
        // the driver and the run suspends waiting for its completion.
        assert!(matches!(
            run.as_mut().poll(&mut cx),
            std::task::Poll::Pending
        ));
        // Dropping `run` here cancels the execution: the scheduler and the
        // task stacks are freed while the driver still holds the submitted
        // host-call future.
    }

    // Polling the retained future to completion must not touch the dropped
    // stacks; it only produces the callback's values as the payload.
    let completion = driver
        .next_completion()
        .await
        .expect("retained host call must still complete");
    assert!(matches!(
        completion.payload,
        CompletionPayload::HostCall { .. }
    ));
    assert_eq!(state.calls.load(Ordering::SeqCst), 1);

    // The same driver remains usable for a fresh run, whose scheduler
    // applies the retained values protocol and produces the result.
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
    assert_eq!(state.calls.load(Ordering::SeqCst), 2);
}

#[cfg(feature = "threads")]
struct SharedMemoryState {
    direct_some: AtomicUsize,
    closure_some: AtomicUsize,
}

/// Host callback for the shared-memory regression test: reads a byte from
/// the caller's shared memory and writes a marker byte back, both through
/// the closure-based accessor.
#[cfg(feature = "threads")]
fn fill_shared_caller_memory(cx: &mut HostContext<'_, '_>) -> VMResult<Vec<WasmValue>> {
    let state = cx.store().state;
    let (WasmValue::I32(src), WasmValue::I32(dst)) = (cx.args()[0], cx.args()[1]) else {
        return VMResult::Unreachable;
    };
    let state = unsafe { state.get::<SharedMemoryState>() }.unwrap();
    // The direct accessor cannot hand out a shared memory as `&mut Memory`.
    if cx.caller_memory().is_some() {
        state.direct_some.fetch_add(1, Ordering::SeqCst);
    }
    let byte = cx.with_caller_memory(|memory| {
        let byte = memory.get(src as usize).copied().unwrap_or(0);
        memory
            .get_mut(dst as usize..dst as usize + 1)
            .expect("marker write must be in bounds")[0] = 0x2A;
        byte
    });
    match byte {
        Some(byte) => {
            state.closure_some.fetch_add(1, Ordering::SeqCst);
            VMResult::Success(vec![WasmValue::I32(i32::from(byte))])
        }
        None => VMResult::Unreachable,
    }
}

/// Regression test for the #216 review (shared-memory regression): a guest
/// whose default memory is shared must still be able to pass pointers to
/// host callbacks. `HostContext::caller_memory` returns `None` for a shared
/// caller memory, so the closure-based `with_caller_memory` accessor —
/// equivalent to the raw surface's pre-#216 accessor — must dispatch to the
/// shared memory under its lock.
#[cfg(feature = "threads")]
#[tokio::test]
async fn host_callback_reaches_shared_caller_memory_via_closure() {
    let state = Box::leak(Box::new(SharedMemoryState {
        direct_some: AtomicUsize::new(0),
        closure_some: AtomicUsize::new(0),
    }));
    let store = Store::new_with_state(StoreState::from_static(state));
    let mut registry = Registry::new();
    let host = instantiate_native_module(
        HostModule {
            functions: vec![HostFnDefinition {
                name: Some("fill".to_owned()),
                signature: FuncType::new(vec![ValType::I32, ValType::I32], vec![ValType::I32]),
                fp: fill_shared_caller_memory,
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
          (import "host" "fill" (func $fill (param i32 i32) (result i32)))
          (memory 1 2 shared)
          (func (export "run") (result i32)
            (i32.store8 (i32.const 0) (i32.const 7))
            (i32.add
              (i32.mul (call $fill (i32.const 0) (i32.const 16)) (i32.const 256))
              (i32.load8_u (i32.const 16)))))
        (assert_return (invoke "run") (i32.const 1834))
        "#,
        &store,
        &mut registry,
    )
    .await;
    // The host read the byte 7 at address 0 and wrote 0x2A at address 16,
    // which the guest then observed: 7 * 256 + 42 = 1834.
    assert_eq!(state.direct_some.load(Ordering::SeqCst), 0);
    assert_eq!(state.closure_some.load(Ordering::SeqCst), 1);
}
