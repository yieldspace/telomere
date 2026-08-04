mod common;
use common::run_wast_with;
use std::sync::atomic::{AtomicUsize, Ordering};
use telomere::{
    component_support::common::{FuncType, ValType},
    host_abi::{
        instantiate_native_module, ExecuteContext, HostFunctionDefinition, Instr, NativeModule,
        ObjectRef,
    },
    link_host_function_with_function_idx, run_module_function,
    unstable_internals::{
        function_call, function_code_pointer, function_host_code_pointer, function_is_host,
        function_locals_size, Operand,
    },
    vm_try, Registry, ResultValue, Store, StoreState, VMResult, WasmValue,
};

fn print_counter<'a>(ctx: &'a ExecuteContext<'a>) -> &'a AtomicUsize {
    unsafe { ctx.store.state.get::<AtomicUsize>() }
        .expect("host function tests require an AtomicUsize in StoreState")
}

fn print(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    print_counter(ctx).fetch_add(1, Ordering::SeqCst);
    let (prev_local_ref, return_addr) =
        ctx.stack
            .function_return_in_place(&ctx.local_reference, 0, ctx.gc);
    ctx.set_local_reference(prev_local_ref);
    VMResult::Success(return_addr)
}

fn return_42(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    ctx.return_slot().write(&42_i32.to_le_bytes());
    let (prev_local_ref, return_addr) =
        ctx.stack
            .function_return_in_place(&ctx.local_reference, 4, ctx.gc);
    ctx.set_local_reference(prev_local_ref);
    VMResult::Success(return_addr)
}

fn return_i64_pair(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    let mut bytes = [0; 16];
    bytes[..8].copy_from_slice(&7_i64.to_le_bytes());
    bytes[8..].copy_from_slice(&(-9_i64).to_le_bytes());
    ctx.return_slot().write(&bytes);
    let (prev_local_ref, return_addr) =
        ctx.stack
            .function_return_in_place(&ctx.local_reference, bytes.len(), ctx.gc);
    ctx.set_local_reference(prev_local_ref);
    VMResult::Success(return_addr)
}

#[tokio::test]
async fn host_without_params_can_return_i32() {
    let store = Store::new();
    let mut registry = Registry::new();
    let host = instantiate_native_module(
        NativeModule {
            functions: vec![HostFunctionDefinition {
                fp: return_42,
                name: Some("answer".to_string()),
                signature: FuncType::new(vec![], vec![ValType::I32]),
            }],
        },
        &store,
        &registry,
    )
    .await
    .unwrap();
    registry.register("host", host.clone());
    link_host_function_with_function_idx(&host, 0, return_42, &store);

    let wast = r#"
    (module
      (import "host" "answer" (func $answer (result i32)))
      (func (export "call") (result i32) (call $answer))
    )
    (assert_return (invoke "call") (i32.const 42))
    "#;
    run_wast_with(wast, &store, &mut registry).await;
}

#[tokio::test]
async fn host_without_params_can_return_two_i64_values() {
    let store = Store::new();
    let mut registry = Registry::new();
    let host = instantiate_native_module(
        NativeModule {
            functions: vec![HostFunctionDefinition {
                fp: return_i64_pair,
                name: Some("pair".to_string()),
                signature: FuncType::new(vec![], vec![ValType::I64, ValType::I64]),
            }],
        },
        &store,
        &registry,
    )
    .await
    .unwrap();
    registry.register("host", host.clone());
    link_host_function_with_function_idx(&host, 0, return_i64_pair, &store);

    let wast = r#"
    (module
      (import "host" "pair" (func $pair (result i64 i64)))
      (func (export "call") (result i64 i64) (call $pair))
    )
    (assert_return (invoke "call") (i64.const 7) (i64.const -9))
    "#;
    run_wast_with(wast, &store, &mut registry).await;
}

#[tokio::test]
async fn top_level_host_without_params_can_return_i32() {
    let store = Store::new();
    let registry = Registry::new();
    let host = instantiate_native_module(
        NativeModule {
            functions: vec![HostFunctionDefinition {
                fp: return_42,
                name: Some("answer".to_string()),
                signature: FuncType::new(vec![], vec![ValType::I32]),
            }],
        },
        &store,
        &registry,
    )
    .await
    .unwrap();
    link_host_function_with_function_idx(&host, 0, return_42, &store);

    let result = run_module_function(&host, &store, "answer", &ResultValue::new(vec![])).await;
    match result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(42)]));
        }
        other => panic!("top-level host call failed: {other:?}"),
    }

    let sync_result = telomere::component_support::runtime::run_core_export_sync_reentrant(
        &host,
        &store,
        "answer",
        &ResultValue::new(vec![]),
    )
    .expect("synchronous top-level host call should run");
    match sync_result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(42)]));
        }
        other => panic!("synchronous top-level host call failed: {other:?}"),
    }
}

#[tokio::test]
async fn top_level_wasm_function_keeps_declared_locals() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = common::instantiate_wat(
        r#"
        (module
          (func (export "with_local") (result i32) (local i32)
            i32.const 42
            local.set 0
            local.get 0)
        )
        "#,
        &store,
        &registry,
    )
    .await;

    let result =
        run_module_function(&instance, &store, "with_local", &ResultValue::new(vec![])).await;
    match result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(42)]));
        }
        other => panic!("top-level Wasm call failed: {other:?}"),
    }
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

const TAIL_CALL_FUNCTION_RETURN: [Instr; 2] = [
    Instr::from_op(telomere::special_function_return),
    Instr::from_operand(Operand { u32: 4 }),
];

fn tail_call(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    vm_try!(ctx.stack.local_get(&ctx.local_reference(), 0, 4));
    let arg0 = ctx.stack.pop_u32();
    vm_try!(ctx.stack.local_get(&ctx.local_reference(), 4, 4));
    let arg1 = ctx.stack.pop_i32();
    vm_try!(ctx.stack.push_i32(arg1 + 40));
    let func_addr = ObjectRef(arg0);
    let is_host = function_is_host(ctx, func_addr);
    let host_fp = function_host_code_pointer(ctx, func_addr);
    let locals_size = function_locals_size(ctx, func_addr);
    let ptr = function_code_pointer(ctx, func_addr);
    if is_host {
        let f = host_fp.expect("host function must expose a host code pointer");
        let local_reference = vm_try!(function_call(
            ctx,
            4,
            0,
            func_addr,
            TAIL_CALL_FUNCTION_RETURN.as_ptr(),
        ));
        ctx.set_local_reference(local_reference);
        f(ctx)
    } else {
        let local_reference = vm_try!(function_call(
            ctx,
            4,
            locals_size,
            func_addr,
            TAIL_CALL_FUNCTION_RETURN.as_ptr(),
        ));
        ctx.set_local_reference(local_reference);
        let ptr = ptr.expect("wasm function must expose a code pointer");
        VMResult::Success(ptr)
    }
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
