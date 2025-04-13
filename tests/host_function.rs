mod common;
use common::{instantiate_wat, run_wast_with};
use telomere::{
    common::{ExecuteContext, FunctionBody, Instr, LocalState},
    link_host_function_with_function_idx, vm_try, Registry, Store, VMResult,
};
static mut PRINT_CALL: Vec<()> = vec![];
fn print(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    #[allow(static_mut_refs)]
    unsafe {
        PRINT_CALL.push(())
    };
    let st = ctx.local_state.pop().unwrap();
    let return_addr = ctx.stack.function_return(&st.local_reference, 0);

    VMResult::Success(return_addr)
}

#[test]
fn test_print() {
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
    );
    registry.register("host", host);
    link_host_function_with_function_idx(host, 0, print, &mut store);
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
    let arg = ctx.stack.pop_i32();
    vm_try!(ctx.stack.push_i32(arg + 40));
    let funcidx = 1;
    let func_addr = ctx.instance().funcs[funcidx];

    let func = &ctx.store.funcs.0[func_addr as usize];
    match &func.body {
        FunctionBody::Wasm(code) => {
            let local_ref = vm_try!(ctx.stack.function_call(
                4,
                code.local_size(),
                func.instance_addr,
                TAIL_CALL_FUNCTION_RETURN.as_ptr()
            ));
            ctx.local_state.push(LocalState {
                local_reference: local_ref,
                code_addr: func_addr,
            });
            VMResult::Success(code.expr.as_ptr())
        }
        FunctionBody::Host(f) => {
            let local_ref = vm_try!(ctx.stack.function_call(
                4,
                0,
                func.instance_addr,
                TAIL_CALL_FUNCTION_RETURN.as_ptr()
            ));
            ctx.local_state.push(LocalState {
                local_reference: local_ref,
                code_addr: func_addr,
            });
            f(ctx)
        }
    }
}

#[test]
fn test_tail_call_wasm() {
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
    );
    registry.register("host", host);
    link_host_function_with_function_idx(host, 0, tail_call, &mut store);
    let wast = r#"
    (module
      (import "host" "tail_call" (func $tail_call (param i32) (result i32)))
      (func (export "tail_call") (param i32) (result i32) (call $tail_call (local.get 0)))
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
    let st = ctx.local_state.pop().unwrap();
    let return_addr = ctx.stack.function_return(&st.local_reference, 4);

    VMResult::Success(return_addr)
}
#[test]
pub fn test_tail_call_native() {
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
    );
    registry.register("host", host);
    link_host_function_with_function_idx(host, 0, tail_call, &mut store);
    link_host_function_with_function_idx(host, 1, plus60, &mut store);

    let wast = r#"
    (module
      (import "host" "tail_call" (func $tail_call (param i32) (result i32)))
      (func (export "tail_call") (param i32) (result i32) (call $tail_call (local.get 0)))
    )
    (assert_return (invoke "tail_call" (i32.const 2)) (i32.const 102))
    "#;
    run_wast_with(wast, &mut store, &mut registry);
}
