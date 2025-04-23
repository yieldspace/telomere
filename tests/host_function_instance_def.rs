mod common;
use common::run_wast_with;
use telomere::{
    common::{
        ExecuteContext, FuncType, FunctionBody, GcRef, HostFunctionDefinition, Instr, NativeModule,
        ValType,
    },
    link_host_function_with_function_idx,
    runtime::instantiate_native_module,
    vm_try, Registry, Store, VMResult,
};
static mut PRINT_CALL: Vec<()> = vec![];
fn print(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    #[allow(static_mut_refs)]
    unsafe {
        PRINT_CALL.push(())
    };
    let (prev_local_ref, return_addr) = ctx.stack.function_return(&ctx.local_reference, 0);
    ctx.local_reference = prev_local_ref;
    VMResult::Success(return_addr)
}

#[test]
fn test_print() {
    let mut store = Store::new();
    let mut registry = Registry::new();
    let host = instantiate_native_module(
        NativeModule {
            functions: vec![HostFunctionDefinition {
                fp: print,
                name: Some("print".to_string()),
                signature: FuncType::new(vec![], vec![]),
            }],
        },
        &mut store,
        &mut registry,
    )
    .unwrap();
    registry.register("host", host.clone());
    link_host_function_with_function_idx(&host, 0, print, &mut store);
    let wast = r#"
    (module
      (import "host" "print" (func $print))
      (func (export "wasm_print") (call $print))
    )
    (invoke "wasm_print")
    "#;
    run_wast_with(wast, &mut store, &mut registry);
    #[allow(static_mut_refs)]
    unsafe {
        assert_eq!(PRINT_CALL, vec![()]);
    }
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
    let func_addr = GcRef(arg0);
    let func = ctx.func_by_addr(func_addr);
    if func.is_host_func() {
        let f = func.host_code_pointer(&ctx.gc);
        ctx.local_reference = vm_try!(ctx.stack.function_call(
            4,
            0,
            func_addr,
            ctx.local_reference,
            TAIL_CALL_FUNCTION_RETURN.as_ptr()
        ));
        f(ctx)
    } else {
        let (locals_data, code_offset) = func.locals_and_code_offset(&ctx.gc);
        let code_addr = func.body;
        ctx.local_reference = vm_try!(ctx.stack.function_call(
            4,
            locals_data.byte_size(),
            func_addr,
            ctx.local_reference,
            TAIL_CALL_FUNCTION_RETURN.as_ptr()
        ));
        let ptr = unsafe { ctx.gc.get_value::<Instr>(code_addr, code_offset) };
        VMResult::Success(ptr)
    }
}

#[test]
fn test_tail_call_wasm() {
    let mut store = Store::new();
    let mut registry = Registry::new();
    let host = instantiate_native_module(
        NativeModule {
            functions: vec![HostFunctionDefinition {
                fp: tail_call,
                name: Some("tail_call".to_string()),
                signature: FuncType::new(vec![ValType::FuncRef, ValType::I32], vec![ValType::I32]),
            }],
        },
        &mut store,
        &mut registry,
    )
    .unwrap();
    registry.register("host", host.clone());
    link_host_function_with_function_idx(&host, 0, tail_call, &mut store);
    let wast = r#"
    (module
      (import "host" "tail_call" (func $tail_call (param funcref i32) (result i32)))
      (func $plus23 (param i32) (result i32) (i32.add (local.get 0) (i32.const 23)))
      (func (export "tail_call") (param i32) (result i32) (call $tail_call (ref.func $plus23) (local.get 0)))
      (elem declare func $plus23)
    )
    (assert_return (invoke "tail_call" (i32.const 2)) (i32.const 65))
    "#;
    run_wast_with(wast, &mut store, &mut registry);
}

fn plus60(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    vm_try!(ctx.stack.local_get(&ctx.local_reference(), 0, 4));
    let arg = ctx.stack.pop_i32();
    tracing::trace!("{arg}");

    vm_try!(ctx.stack.push_i32(arg + 60));
    let (prev_local_ref, return_addr) = ctx.stack.function_return(&ctx.local_reference, 4);
    ctx.local_reference = prev_local_ref;
    VMResult::Success(return_addr)
}
#[test]
pub fn test_tail_call_native() {
    let mut store = Store::new();
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
        &mut store,
        &mut registry,
    )
    .unwrap();
    registry.register("host", host.clone());
    link_host_function_with_function_idx(&host, 0, tail_call, &mut store);
    link_host_function_with_function_idx(&host, 1, plus60, &mut store);

    let wast = r#"
    (module
      (import "host" "tail_call" (func $tail_call (param funcref i32) (result i32)))
      (import "host" "plus60" (func $plus60 (param i32) (result i32)))
      (func (export "tail_call") (param i32) (result i32) (call $tail_call (ref.func $plus60) (local.get 0)))
      (elem declare func $plus60)
    )
    (assert_return (invoke "tail_call" (i32.const 2)) (i32.const 102))
    "#;
    run_wast_with(wast, &mut store, &mut registry);
}
