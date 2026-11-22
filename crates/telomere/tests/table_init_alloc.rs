mod common;

use common::instantiate_wat;
use telomere::{run_module_function, Registry, ResultValue, Store, VMResult, WasmValue};

/// Module with `func_count` trivial functions, a 4-entry funcref table and a
/// passive element segment holding functions 7 and 9.
///
/// Issue #175: every `table.init` execution used to collect all function
/// addresses of the instance into a fresh `Vec`, so even a zero-length
/// `table.init` cost O(module function count) allocation per call.
fn many_func_module(func_count: usize) -> String {
    let mut wat =
        String::from("(module (type $t (func (result i32))) (table 4 funcref) (elem func $f7 $f9)");
    for i in 0..func_count {
        wat.push_str(&format!(" (func $f{i} (type $t) i32.const {i})"));
    }
    wat.push_str(
        r#" (func (export "init_zero") (param $n i32)
              (local $i i32)
              block $exit
                loop $loop
                  local.get $i
                  local.get $n
                  i32.ge_u
                  br_if $exit
                  i32.const 0
                  i32.const 0
                  i32.const 0
                  table.init 0 0
                  local.get $i
                  i32.const 1
                  i32.add
                  local.set $i
                  br $loop
                end
              end)
            (func (export "init_some")
              i32.const 1
              i32.const 0
              i32.const 2
              table.init 0 0)
            (func (export "call_at") (param $i i32) (result i32)
              local.get $i
              call_indirect (type $t)))"#,
    );
    wat
}

async fn call(
    instance: &telomere::InstanceHandle,
    store: &Store,
    name: &str,
    args: Vec<WasmValue>,
) -> VMResult<ResultValue> {
    run_module_function(instance, store, name, &ResultValue::new(args)).await
}

#[tokio::test]
async fn table_init_zero_len_in_many_func_module() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(&many_func_module(512), &store, &registry).await;

    // 10_000 zero-length table.init executions must succeed and leave the
    // table untouched.
    let result = call(&instance, &store, "init_zero", vec![WasmValue::I32(10_000)]).await;
    assert!(
        matches!(result, VMResult::Success(_)),
        "zero-length table.init loop failed: {result:?}"
    );
}

#[tokio::test]
async fn table_init_nonzero_copies_segment_entries() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(&many_func_module(512), &store, &registry).await;

    let result = call(&instance, &store, "init_some", vec![]).await;
    assert!(
        matches!(result, VMResult::Success(_)),
        "table.init failed: {result:?}"
    );

    // The segment holds functions 7 and 9; they were copied to table
    // slots 1 and 2, and calling through the table must reach them.
    for (slot, expected) in [(1, 7), (2, 9)] {
        let result = call(&instance, &store, "call_at", vec![WasmValue::I32(slot)]).await;
        match result {
            VMResult::Success(values) => {
                assert_eq!(
                    values.iter().next(),
                    Some(&WasmValue::I32(expected)),
                    "table slot {slot} should call function {expected}"
                );
            }
            other => panic!("call_at({slot}) failed: {other:?}"),
        }
    }
}
