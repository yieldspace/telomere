mod common;
use common::{instantiate_wat, run_wast_with};
use telomere::{
    common::{ExecuteContext, Instr},
    link_host_function_with_function_idx, vm_try, Registry, Store, VMResult,
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

#[tokio::test]
async fn test_print() {
    let mut store = Store::new();
    let mut registry = Registry::new();
    let host = instantiate_wat(
        r#"
    (module
      (func (export "print"))
    )
    "#,
        &mut store,
        &registry,
    )
    .await;
    registry.register("host", host.clone());
    link_host_function_with_function_idx(&host, 0, print, &store);
    let wast = r#"
    (module
      (import "host" "print" (func $print))
      (func (export "wasm_print") (call $print))
    )
    (invoke "wasm_print")
    "#;
    run_wast_with(wast, &mut store, &mut registry).await;
    #[allow(static_mut_refs)]
    unsafe {
        assert_eq!(PRINT_CALL, vec![()]);
    }
}

#[tokio::test]
async fn test_imported_host_start() {
    #[allow(static_mut_refs)]
    unsafe {
        PRINT_CALL.clear();
    }

    let mut store = Store::new();
    let mut registry = Registry::new();
    let host = instantiate_wat(
        r#"
    (module
      (func (export "print"))
    )
    "#,
        &mut store,
        &registry,
    )
    .await;
    registry.register("host", host.clone());
    link_host_function_with_function_idx(&host, 0, print, &store);

    let _instance = instantiate_wat(
        r#"
    (module
      (import "host" "print" (func $print))
      (start $print)
    )
    "#,
        &mut store,
        &registry,
    )
    .await;

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
    let arg = ctx.stack.pop_i32();
    vm_try!(ctx.stack.push_i32(arg + 40));
    let funcidx = 1;
    let func_addr = ctx.instance().funcs.as_slice(ctx.gc)[funcidx];

    let func = ctx.func_by_addr(func_addr);
    if func.is_host_func() {
        let fp = func.host_code_pointer(ctx.gc);
        ctx.local_reference = vm_try!(ctx.stack.function_call(
            4,
            0,
            func_addr,
            ctx.local_reference,
            TAIL_CALL_FUNCTION_RETURN.as_ptr()
        ));
        fp(ctx)
    } else {
        let (locals_data, code_offset) = func.locals_and_code_offset(ctx.gc);
        let instr = unsafe { ctx.gc.get_value::<Instr>(func.body, code_offset) };
        ctx.local_reference = vm_try!(ctx.stack.function_call(
            4,
            locals_data.byte_size(),
            func_addr,
            ctx.local_reference,
            TAIL_CALL_FUNCTION_RETURN.as_ptr()
        ));
        VMResult::Success(instr)
    }
}

#[tokio::test]
async fn test_tail_call_wasm() {
    let mut store = Store::new();
    let mut registry = Registry::new();
    let host = instantiate_wat(
        r#"
    (module
      (func (export "tail_call") (param i32) (result i32) (unreachable))
      (func $plus23 (param i32) (result i32) (i32.add (local.get 0) (i32.const 23)))
    )
    "#,
        &mut store,
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
    run_wast_with(wast, &mut store, &mut registry).await;
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
#[tokio::test]
pub async fn test_tail_call_native() {
    let mut store = Store::new();
    let mut registry = Registry::new();
    let host = instantiate_wat(
        r#"
    (module
      (func (export "tail_call") (param i32) (result i32) (unreachable))
      (func (param i32) (result i32) (unreachable))
    )
    "#,
        &mut store,
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
    run_wast_with(wast, &mut store, &mut registry).await;
}
