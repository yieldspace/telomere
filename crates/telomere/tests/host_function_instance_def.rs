mod common;
use common::run_wast_with;
use std::sync::atomic::{AtomicUsize, Ordering};
use telomere::{
    common::{
        ExecuteContext, FuncType, HostFunctionDefinition, Instr, NativeModule, ObjectRef,
        StoreState, ValType,
    },
    link_host_function_with_function_idx,
    runtime::instantiate_native_module,
    vm_try, Registry, Store, VMResult,
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
    Instr {
        op: telomere::special_function_return,
    },
    Instr {
        operand: telomere::common::Operand { u32: 4 },
    },
];

fn tail_call(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    vm_try!(ctx.stack.local_get(&ctx.local_reference(), 0, 4));
    let arg0 = ctx.stack.pop_u32();
    vm_try!(ctx.stack.local_get(&ctx.local_reference(), 4, 4));
    let arg1 = ctx.stack.pop_i32();
    vm_try!(ctx.stack.push_i32(arg1 + 40));
    let func_addr = ObjectRef(arg0);
    let (is_host, host_fp, locals_size, ptr) = {
        let func = ctx.func_by_addr(func_addr);
        (
            func.is_host_func(),
            func.is_host_func().then(|| func.host_code_pointer()),
            func.locals().byte_size(),
            func.code_pointer(),
        )
    };
    if is_host {
        let f = host_fp.expect("host function must expose a host code pointer");
        let local_reference = vm_try!(ctx.stack.function_call(
            4,
            0,
            func_addr,
            ctx.local_reference,
            TAIL_CALL_FUNCTION_RETURN.as_ptr(),
            ctx.gc,
        ));
        ctx.set_local_reference(local_reference);
        f(ctx)
    } else {
        let local_reference = vm_try!(ctx.stack.function_call(
            4,
            locals_size,
            func_addr,
            ctx.local_reference,
            TAIL_CALL_FUNCTION_RETURN.as_ptr(),
            ctx.gc,
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

fn const7(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    let slot = ctx.return_slot();
    slot.write(&7i32.to_le_bytes());
    let (prev_local_ref, return_addr) =
        ctx.stack
            .function_return_in_place(&ctx.local_reference, 4, ctx.gc);
    ctx.set_local_reference(prev_local_ref);
    VMResult::Success(return_addr)
}

#[tokio::test]
pub async fn test_zero_param_native_result() {
    let store = Store::new();
    let mut registry = Registry::new();
    let host = instantiate_native_module(
        NativeModule {
            functions: vec![HostFunctionDefinition {
                fp: const7,
                name: Some("const7".to_string()),
                signature: FuncType::new(vec![], vec![ValType::I32]),
            }],
        },
        &store,
        &registry,
    )
    .await
    .unwrap();
    registry.register("host", host.clone());
    link_host_function_with_function_idx(&host, 0, const7, &store);

    let wast = r#"
    (module
      (import "host" "const7" (func $const7 (result i32)))
      (func (export "const7") (result i32) (call $const7))
    )
    (assert_return (invoke "const7") (i32.const 7))
    "#;
    run_wast_with(wast, &store, &mut registry).await;
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
