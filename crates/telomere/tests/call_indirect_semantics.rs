mod common;

use common::instantiate_wat;
use telomere::{run_module_function, Registry, ResultValue, Store, VMResult, WasmValue};

#[tokio::test]
async fn call_indirect_traps_on_table_index_out_of_range() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (type $sig (func (result i32)))
          (table 1 funcref)
          (elem (i32.const 0) func $callee)
          (func $callee (type $sig)
            i32.const 7)
          (func (export "f") (param i32) (result i32)
            local.get 0
            call_indirect (type $sig)))
        "#,
        &store,
        &registry,
    )
    .await;

    assert!(matches!(
        run_module_function(
            &instance,
            &store,
            "f",
            &ResultValue::new(vec![WasmValue::I32(1)]),
        )
        .await,
        VMResult::TableIndexOutOfRange
    ));
}

#[tokio::test]
async fn call_indirect_traps_on_uninitialized_table_entry() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (type $sig (func (result i32)))
          (table 2 funcref)
          (elem (i32.const 0) func $callee)
          (func $callee (type $sig)
            i32.const 7)
          (func (export "f") (param i32) (result i32)
            local.get 0
            call_indirect (type $sig)))
        "#,
        &store,
        &registry,
    )
    .await;

    assert!(matches!(
        run_module_function(
            &instance,
            &store,
            "f",
            &ResultValue::new(vec![WasmValue::I32(1)]),
        )
        .await,
        VMResult::TableUninitialized
    ));
}

#[tokio::test]
async fn call_indirect_traps_on_type_mismatch() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (type $expected (func (result i32)))
          (type $actual (func (param i32) (result i32)))
          (table 1 funcref)
          (elem (i32.const 0) func $wrong)
          (func $wrong (type $actual)
            local.get 0)
          (func (export "f") (result i32)
            i32.const 0
            call_indirect (type $expected)))
        "#,
        &store,
        &registry,
    )
    .await;

    assert!(matches!(
        run_module_function(&instance, &store, "f", &ResultValue::new(vec![])).await,
        VMResult::CallIndirectInvalidType
    ));
}
