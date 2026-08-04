mod common;
use common::{instantiate_wat, run_wast_with};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
};
use telomere::{
    host_abi::{ExecuteContext, Instr},
    link_host_function_with_export_name, link_host_function_with_function_idx, run_module_function,
    unstable_internals::{
        function_address, function_call, function_code_pointer, function_host_code_pointer,
        function_is_host, function_locals_size, Operand,
    },
    vm_try, InstanceHandle, Registry, ResultValue, Store, StoreState, VMResult,
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

fn link_state<'a>(ctx: &'a ExecuteContext<'a>) -> &'a LinkState {
    unsafe { ctx.store.state.get::<LinkState>() }
        .expect("host function tests require LinkState in StoreState")
}

fn print_counter<'a>(ctx: &'a ExecuteContext<'a>) -> &'a AtomicUsize {
    &link_state(ctx).counter
}

fn finish_host_call(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    let (prev_local_ref, return_addr) =
        ctx.stack
            .function_return_in_place(&ctx.local_reference, 0, ctx.gc);
    ctx.set_local_reference(prev_local_ref);
    VMResult::Success(return_addr)
}

fn host_instance(ctx: &ExecuteContext) -> InstanceHandle {
    link_state(ctx)
        .host
        .lock()
        .unwrap()
        .clone()
        .expect("host instance must be recorded before invoking the test host function")
}

fn print(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    print_counter(ctx).fetch_add(1, Ordering::SeqCst);
    finish_host_call(ctx)
}

fn relink_by_function_idx(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    let host = host_instance(ctx);
    link_host_function_with_function_idx(&host, 0, print, ctx.store);
    finish_host_call(ctx)
}

fn relink_by_export_name(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    let host = host_instance(ctx);
    link_host_function_with_export_name(&host, "print", print, ctx.store);
    finish_host_call(ctx)
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
async fn test_print_by_export_name() {
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
    link_host_function_with_export_name(&host, "print", print, &store);
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
async fn test_imported_call_stays_dynamic_after_caller_instantiation() {
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

    let caller = instantiate_wat(
        r#"
    (module
      (import "host" "print" (func $print))
      (func (export "wasm_print") (call $print))
    )
    "#,
        &store,
        &registry,
    )
    .await;

    link_host_function_with_function_idx(&host, 0, print, &store);
    let result =
        run_module_function(&caller, &store, "wasm_print", &ResultValue::new(vec![])).await;
    assert!(matches!(result, VMResult::Success(values) if values.is_empty()));
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

const TAIL_CALL_FUNCTION_RETURN: [Instr; 2] = [
    Instr::from_op(telomere::special_function_return),
    Instr::from_operand(Operand { u32: 4 }),
];

fn tail_call(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    vm_try!(ctx.stack.local_get(&ctx.local_reference(), 0, 4));
    let arg = ctx.stack.pop_i32();
    vm_try!(ctx.stack.push_i32(arg + 40));
    let func_addr = function_address(ctx, 1).expect("function index must resolve to an address");
    let is_host = function_is_host(ctx, func_addr);
    let host_fp = function_host_code_pointer(ctx, func_addr);
    let locals_size = function_locals_size(ctx, func_addr);
    let instr = function_code_pointer(ctx, func_addr);
    if is_host {
        let fp = host_fp.expect("host function must expose a host code pointer");
        let local_reference = vm_try!(function_call(
            ctx,
            4,
            0,
            func_addr,
            TAIL_CALL_FUNCTION_RETURN.as_ptr(),
        ));
        ctx.set_local_reference(local_reference);
        fp(ctx)
    } else {
        let instr = instr.expect("wasm function must expose a code pointer");
        let local_reference = vm_try!(function_call(
            ctx,
            4,
            locals_size,
            func_addr,
            TAIL_CALL_FUNCTION_RETURN.as_ptr(),
        ));
        ctx.set_local_reference(local_reference);
        VMResult::Success(instr)
    }
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

fn plus60(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    let value = i32::from_le_bytes(
        ctx.stack
            .local_bytes(&ctx.local_reference(), 0, 4)
            .try_into()
            .unwrap(),
    );
    tracing::trace!("{value}");
    let slot = ctx.return_slot();
    slot.write(&(value + 60).to_le_bytes());
    let (prev_local_ref, return_addr) =
        ctx.stack
            .function_return_in_place(&ctx.local_reference, 4, ctx.gc);
    ctx.set_local_reference(prev_local_ref);
    VMResult::Success(return_addr)
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
