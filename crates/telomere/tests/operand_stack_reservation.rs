//! #186: frame entry reserves the validated maximum operand depth, so a
//! function whose validated operand bound exceeds the stack capacity traps
//! with `StackOverflow` at call entry — before any of its code runs — while
//! functions whose bound fits execute with unchecked pushes.

mod common;

use common::instantiate_wat;
use telomere::{run_module_function, Registry, ResultValue, Store, VMResult, WasmValue};

/// Builds a module whose exported `deep` function has an `if` whose
/// then-branch pushes `pushes` i32 values (and drops all but one). The
/// validated operand peak of `deep` is `pushes * 4` bytes regardless of
/// which branch is taken at runtime.
fn deep_module(pushes: usize) -> String {
    let mut body = String::new();
    for _ in 0..pushes {
        body.push_str("i32.const 1\n");
    }
    for _ in 0..pushes - 1 {
        body.push_str("drop\n");
    }
    format!(
        r#"(module
             (func (export "deep") (param i32) (result i32)
               local.get 0
               if (result i32)
                 {body}
               else
                 i32.const 7
               end)
             (func (export "recurse") (param i32) (result i32)
               local.get 0
               call 1))"#
    )
}

async fn call_deep(wat: &str, arg: i32) -> VMResult<ResultValue> {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(wat, &store, &registry).await;
    run_module_function(
        &instance,
        &store,
        "deep",
        &ResultValue::new(vec![WasmValue::I32(arg)]),
    )
    .await
}

#[tokio::test]
async fn operand_bound_beyond_stack_traps_at_entry_even_on_the_shallow_path() {
    // 40_000 i32 values = 160_000 operand bytes, beyond the default
    // 128 KiB execution stack. The condition selects the shallow branch,
    // which by itself would return 7 without ever approaching that depth:
    // before #186 this call succeeded, because the stack only grew along
    // the executed path. The entry reservation makes it trap up front.
    let result = call_deep(&deep_module(40_000), 0).await;
    assert!(
        matches!(result, VMResult::StackOverflow),
        "expected StackOverflow at frame entry, got {result:?}"
    );
}

#[tokio::test]
async fn operand_bound_within_stack_executes_both_branches() {
    // 100 i32 values = 400 operand bytes: comfortably inside the stack, so
    // both branches run — the shallow one returns 7, the deep one executes
    // its unchecked pushes and returns the surviving value.
    let shallow = call_deep(&deep_module(100), 0).await;
    match shallow {
        VMResult::Success(values) => match values.iter().next() {
            Some(WasmValue::I32(value)) => assert_eq!(*value, 7),
            other => panic!("expected i32 result 7, got {other:?}"),
        },
        other => panic!("shallow branch must succeed, got {other:?}"),
    }
    let deep = call_deep(&deep_module(100), 1).await;
    match deep {
        VMResult::Success(values) => match values.iter().next() {
            Some(WasmValue::I32(value)) => assert_eq!(*value, 1),
            other => panic!("expected i32 result 1, got {other:?}"),
        },
        other => panic!("deep branch must succeed, got {other:?}"),
    }
}

#[tokio::test]
async fn deep_recursion_still_traps_with_stack_overflow() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(&deep_module(4), &store, &registry).await;
    let result = run_module_function(
        &instance,
        &store,
        "recurse",
        &ResultValue::new(vec![WasmValue::I32(0)]),
    )
    .await;
    assert!(
        matches!(result, VMResult::StackOverflow),
        "expected StackOverflow from unbounded recursion, got {result:?}"
    );
}
