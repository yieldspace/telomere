mod common;

use common::instantiate_wat;
use std::sync::Mutex;
use telomere::{
    component_support::{
        common::{memory_export, read_memory},
        runtime::run_core_export_sync_reentrant,
    },
    get_global,
    host_abi::{ExecuteContext, Instr},
    link_host_function_with_function_idx, run_module_function, InstanceHandle, IoReadBinaryReader,
    Registry, ResultValue, Store, StoreState, VMResult, WasmParser, WasmParserError, WasmValue,
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

struct ReentrantGetGlobalState {
    instance: Mutex<Option<InstanceHandle>>,
}

fn return_from_host(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    let (previous_local_reference, return_address) =
        ctx.stack
            .function_return_in_place(&ctx.local_reference, 0, ctx.gc);
    ctx.set_local_reference(previous_local_reference);
    VMResult::Success(return_address)
}

fn get_global_from_host_callback(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    let state = unsafe { ctx.store.state.get::<ReentrantGetGlobalState>() }
        .expect("reentrant get_global test requires ReentrantGetGlobalState in StoreState");
    let instance = state
        .instance
        .lock()
        .expect("reentrant get_global instance mutex must not be poisoned")
        .clone()
        .expect("reentrant get_global instance must be installed before guest execution");

    assert!(matches!(
        get_global(&instance, ctx.store, "g"),
        VMResult::Unlinkable
    ));
    return_from_host(ctx)
}

#[tokio::test]
async fn get_global_fails_closed_during_host_callback() {
    let state = Box::leak(Box::new(ReentrantGetGlobalState {
        instance: Mutex::new(None),
    }));
    let store = Store::new_with_state(StoreState::from_static(state));
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (global (export "g") i32 (i32.const 7))
          (func $host)
          (func (export "run") call $host))
        "#,
        &store,
        &registry,
    )
    .await;
    *state
        .instance
        .lock()
        .expect("reentrant get_global instance mutex must not be poisoned") =
        Some(instance.clone());
    link_host_function_with_function_idx(&instance, 0, get_global_from_host_callback, &store);

    assert!(matches!(
        run_module_function(&instance, &store, "run", &ResultValue::new(vec![])).await,
        VMResult::Success(values) if values.is_empty()
    ));
}

#[tokio::test]
async fn component_memory_read_rejects_lengths_outside_wasm32_address_space() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory (export "memory") 1)
          (data (i32.const 4) "\01\02\03"))
        "#,
        &store,
        &registry,
    )
    .await;
    let memory = memory_export(&instance, &store, "memory").expect("memory export must exist");

    assert_eq!(read_memory(&store, &memory, 4, 3), Some(vec![1, 2, 3]));

    #[cfg(target_pointer_width = "64")]
    assert_eq!(read_memory(&store, &memory, 4, u32::MAX as usize + 1), None);
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

#[cfg(feature = "simd")]
#[tokio::test]
async fn simd_replace_lane_value_flows_through_global_set_and_return() {
    const F32_3_14: f32 = f32::from_bits(0x4048_f5c3);
    const F64_3_14: f64 = f64::from_bits(0x4009_1eb8_51eb_851f);

    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (global $g (export "g") (mut v128) (v128.const f32x4 0 0 0 0))
          (func (export "return-f32-param") (param v128 f32) (result f32)
            (return (local.get 1)))
          (func (export "replace-f32-lane-only") (param v128 f32) (result v128)
            (return (f32x4.replace_lane 0 (local.get 0) (local.get 1))))
          (func (export "as-global_set-value-1") (param v128 f32) (result v128)
            (global.set $g (f32x4.replace_lane 0 (local.get 0) (local.get 1)))
            (return (global.get $g)))
          (func (export "return-f64-param") (param v128 f64) (result f64)
            (return (local.get 1)))
          (func (export "replace-f64-lane-only") (param v128 f64) (result v128)
            (return (f64x2.replace_lane 0 (local.get 0) (local.get 1))))
          (func (export "as-global_set-value-3") (param v128 f64) (result v128)
            (global.set $g (f64x2.replace_lane 0 (local.get 0) (local.get 1)))
            (return (global.get $g))))
        "#,
        &store,
        &registry,
    )
    .await;

    let f32_param = run_module_function(
        &instance,
        &store,
        "return-f32-param",
        &ResultValue::new(vec![WasmValue::V128(0), WasmValue::F32(F32_3_14)]),
    )
    .await
    .unwrap();
    assert_eq!(f32_param, ResultValue::new(vec![WasmValue::F32(F32_3_14)]));

    let f32_lane_only = run_module_function(
        &instance,
        &store,
        "replace-f32-lane-only",
        &ResultValue::new(vec![WasmValue::V128(0), WasmValue::F32(F32_3_14)]),
    )
    .await
    .unwrap();
    assert_eq!(
        f32_lane_only,
        ResultValue::new(vec![WasmValue::V128(u128::from_le_bytes([
            0xc3, 0xf5, 0x48, 0x40, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ]))])
    );

    let f32_result = run_module_function(
        &instance,
        &store,
        "as-global_set-value-1",
        &ResultValue::new(vec![WasmValue::V128(0), WasmValue::F32(F32_3_14)]),
    )
    .await
    .unwrap();
    let f32_global = get_global(&instance, &store, "g").unwrap();
    assert_eq!(
        f32_result,
        ResultValue::new(vec![WasmValue::V128(u128::from_le_bytes([
            0xc3, 0xf5, 0x48, 0x40, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ]))])
    );
    assert_eq!(
        f32_global,
        WasmValue::V128(u128::from_le_bytes([
            0xc3, 0xf5, 0x48, 0x40, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ]))
    );

    let f64_param = run_module_function(
        &instance,
        &store,
        "return-f64-param",
        &ResultValue::new(vec![WasmValue::V128(0), WasmValue::F64(F64_3_14)]),
    )
    .await
    .unwrap();
    assert_eq!(f64_param, ResultValue::new(vec![WasmValue::F64(F64_3_14)]));

    let f64_lane_only = run_module_function(
        &instance,
        &store,
        "replace-f64-lane-only",
        &ResultValue::new(vec![WasmValue::V128(0), WasmValue::F64(F64_3_14)]),
    )
    .await
    .unwrap();
    assert_eq!(
        f64_lane_only,
        ResultValue::new(vec![WasmValue::V128(u128::from_le_bytes([
            0x1f, 0x85, 0xeb, 0x51, 0xb8, 0x1e, 0x09, 0x40, 0, 0, 0, 0, 0, 0, 0, 0,
        ]))])
    );

    let f64_result = run_module_function(
        &instance,
        &store,
        "as-global_set-value-3",
        &ResultValue::new(vec![WasmValue::V128(0), WasmValue::F64(F64_3_14)]),
    )
    .await
    .unwrap();
    assert_eq!(
        f64_result,
        ResultValue::new(vec![WasmValue::V128(u128::from_le_bytes([
            0x1f, 0x85, 0xeb, 0x51, 0xb8, 0x1e, 0x09, 0x40, 0, 0, 0, 0, 0, 0, 0, 0,
        ]))])
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
