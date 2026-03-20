mod common;

use common::instantiate_wat;
use telomere::{
    component_support::runtime::run_core_export_sync_reentrant, get_global, run_module_function,
    IoReadBinaryReader, Registry, ResultValue, Store, VMResult, WasmParser, WasmParserError,
    WasmValue,
};

fn parse_module_err(wat: &str) -> WasmParserError {
    let source = wat::parse_str(wat).expect("wat must parse");
    let mut reader = IoReadBinaryReader::from(&source[..]);
    let mut parser = WasmParser::new(&mut reader);
    match parser.parse_module() {
        Ok(_) => panic!("module must fail to parse"),
        Err(err) => err,
    }
}

#[tokio::test]
async fn get_global_supports_core_value_types() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (global (export "g_i32") i32 (i32.const 42))
          (global (export "g_i64") i64 (i64.const 43))
          (global (export "g_f32") f32 (f32.const 44.5))
          (global (export "g_f64") f64 (f64.const 45.5))
          (global (export "g_funcref") funcref (ref.null func))
          (global (export "g_externref") externref (ref.null extern)))
        "#,
        &store,
        &registry,
    )
    .await;

    assert_eq!(
        get_global(&instance, &store, "g_i32").unwrap(),
        WasmValue::I32(42)
    );
    assert_eq!(
        get_global(&instance, &store, "g_i64").unwrap(),
        WasmValue::I64(43)
    );
    assert_eq!(
        get_global(&instance, &store, "g_f32").unwrap(),
        WasmValue::F32(44.5)
    );
    assert_eq!(
        get_global(&instance, &store, "g_f64").unwrap(),
        WasmValue::F64(45.5)
    );
    assert_eq!(
        get_global(&instance, &store, "g_funcref").unwrap(),
        WasmValue::FuncRef(0)
    );
    assert_eq!(
        get_global(&instance, &store, "g_externref").unwrap(),
        WasmValue::ExternRef(0)
    );
}

#[cfg(feature = "simd")]
#[tokio::test]
async fn get_global_supports_v128() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (global (export "g_v128") v128 (v128.const i32x4 1 2 3 4)))
        "#,
        &store,
        &registry,
    )
    .await;

    assert_eq!(
        get_global(&instance, &store, "g_v128").unwrap(),
        WasmValue::V128(u128::from_le_bytes([
            1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 4, 0, 0, 0,
        ]))
    );
}

#[tokio::test]
async fn public_runtime_apis_fail_closed_for_missing_or_wrong_export_kind() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (global (export "g") i32 (i32.const 7))
          (func (export "f") (result i32) (i32.const 8)))
        "#,
        &store,
        &registry,
    )
    .await;

    assert!(matches!(
        get_global(&instance, &store, "missing"),
        VMResult::Unlinkable
    ));
    assert!(matches!(
        get_global(&instance, &store, "f"),
        VMResult::Unlinkable
    ));
    assert!(matches!(
        run_module_function(&instance, &store, "missing", &ResultValue::new(vec![])).await,
        VMResult::Unlinkable
    ));
    assert!(matches!(
        run_module_function(&instance, &store, "g", &ResultValue::new(vec![])).await,
        VMResult::Unlinkable
    ));
    assert!(matches!(
        run_core_export_sync_reentrant(&instance, &store, "missing", &ResultValue::new(vec![])),
        Ok(VMResult::Unlinkable)
    ));
    assert!(matches!(
        run_core_export_sync_reentrant(&instance, &store, "g", &ResultValue::new(vec![])),
        Ok(VMResult::Unlinkable)
    ));
}

#[tokio::test]
async fn declarative_const_expr_ref_func_marks_function_declared() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (elem declare funcref (ref.func $f))
          (func $f (export "use_declared")
            ref.func $f
            drop))
        "#,
        &store,
        &registry,
    )
    .await;

    assert!(matches!(
        run_module_function(&instance, &store, "use_declared", &ResultValue::new(vec![])).await,
        VMResult::Success(_)
    ));
}

#[test]
fn declarative_const_expr_without_ref_func_does_not_declare_function() {
    let err = parse_module_err(
        r#"
        (module
          (elem declare funcref (ref.null func))
          (func $f (export "use_declared")
            ref.func $f
            drop))
        "#,
    );
    assert!(matches!(err, WasmParserError::UndeclaredFunctionReference));
}
