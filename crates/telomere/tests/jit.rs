use telomere::{JitConfig, RuntimeConfig, Store};
#[cfg(all(feature = "jit", target_os = "macos", target_arch = "aarch64"))]
use telomere::{Registry, ResultValue, VMResult, WasmValue};

#[cfg(all(feature = "jit", target_os = "macos", target_arch = "aarch64"))]
fn parse_module(wat: &str) -> telomere::Module {
    let bytes = wat::parse_str(wat).expect("wat should parse");
    let mut reader = telomere::IoReadBinaryReader::from(&bytes[..]);
    let mut parser = telomere::WasmParser::new(&mut reader);
    parser.parse_module().expect("module should parse")
}

#[test]
fn runtime_config_defaults_to_jit_off() {
    assert!(!Store::new().runtime_config().jit.enabled);

    let store = Store::new_with_runtime_config(RuntimeConfig {
        jit: JitConfig {
            enabled: true,
            ..JitConfig::default()
        },
    });
    assert!(store.runtime_config().jit.enabled);
    assert_eq!(
        store.runtime_config().jit.code_cache_max_bytes,
        4 * 1024 * 1024
    );
}

#[cfg(all(feature = "jit", target_os = "macos", target_arch = "aarch64"))]
fn jit_store() -> Store {
    Store::new_with_runtime_config(RuntimeConfig {
        jit: JitConfig {
            enabled: true,
            ..JitConfig::default()
        },
    })
}

#[cfg(all(feature = "jit", target_os = "macos", target_arch = "aarch64"))]
async fn invoke_jit(wat: &str, name: &str, args: Vec<WasmValue>) -> VMResult<ResultValue> {
    let module = parse_module(wat);
    let store = jit_store();
    let registry = Registry::new();
    let instance = match telomere::instantiate(module, &store, &registry).await {
        VMResult::Success(instance) => instance,
        other => return other.map(|_| unreachable!()),
    };
    telomere::run_module_function(&instance, &store, name, &ResultValue::new(args)).await
}

#[cfg(all(feature = "jit", target_os = "macos", target_arch = "aarch64"))]
fn assert_success_i32(result: VMResult<ResultValue>, expected: i32) {
    let VMResult::Success(values) = result else {
        panic!("expected success, got {result:?}");
    };
    assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
}

#[cfg(all(feature = "jit", target_os = "macos", target_arch = "aarch64"))]
trait VmResultMap<T> {
    fn map<U>(self, f: impl FnOnce(T) -> U) -> VMResult<U>;
}

#[cfg(all(feature = "jit", target_os = "macos", target_arch = "aarch64"))]
impl<T> VmResultMap<T> for VMResult<T> {
    fn map<U>(self, f: impl FnOnce(T) -> U) -> VMResult<U> {
        match self {
            VMResult::Success(value) => VMResult::Success(f(value)),
            VMResult::Unreachable => VMResult::Unreachable,
            VMResult::StackOverflow => VMResult::StackOverflow,
            VMResult::MemoryIndexOutOfRange => VMResult::MemoryIndexOutOfRange,
            VMResult::TableIndexOutOfRange => VMResult::TableIndexOutOfRange,
            VMResult::CallIndirectInvalidType => VMResult::CallIndirectInvalidType,
            VMResult::TableUninitialized => VMResult::TableUninitialized,
            VMResult::Unlinkable => VMResult::Unlinkable,
            VMResult::InvalidOperand => VMResult::InvalidOperand,
            VMResult::UnalignedAtomic => VMResult::UnalignedAtomic,
            VMResult::Unimplemented => VMResult::Unimplemented,
        }
    }
}

#[cfg(all(feature = "jit", target_os = "macos", target_arch = "aarch64"))]
#[tokio::test]
async fn jit_executes_i32_locals_and_arithmetic() {
    let result = invoke_jit(
        r#"
        (module
          (func (export "calc") (param i32) (result i32)
            (local i32)
            local.get 0
            i32.const 7
            i32.add
            local.set 1
            local.get 1
            i32.const 5
            i32.mul))
        "#,
        "calc",
        vec![WasmValue::I32(8)],
    )
    .await;

    assert_success_i32(result, 75);
}

#[cfg(all(feature = "jit", target_os = "macos", target_arch = "aarch64"))]
#[tokio::test]
async fn jit_direct_call_enters_callee() {
    let result = invoke_jit(
        r#"
        (module
          (func $add1 (param i32) (result i32)
            local.get 0
            i32.const 1
            i32.add)
          (func (export "run") (param i32) (result i32)
            local.get 0
            call $add1))
        "#,
        "run",
        vec![WasmValue::I32(41)],
    )
    .await;

    assert_success_i32(result, 42);
}

#[cfg(all(feature = "jit", target_os = "macos", target_arch = "aarch64"))]
#[tokio::test]
async fn jit_direct_call_result_is_available_to_continuation() {
    let result = invoke_jit(
        r#"
        (module
          (func $add1 (param i32) (result i32)
            local.get 0
            i32.const 1
            i32.add)
          (func (export "run") (param i32) (result i32)
            local.get 0
            call $add1
            i32.const 2
            i32.add))
        "#,
        "run",
        vec![WasmValue::I32(41)],
    )
    .await;

    assert_success_i32(result, 44);
}

#[cfg(all(feature = "jit", target_os = "macos", target_arch = "aarch64"))]
#[tokio::test]
async fn jit_br_if_fallback_preserves_fallthrough_stack() {
    let result = invoke_jit(
        r#"
        (module
          (func (export "run") (param i32) (result i32)
            (block (result i32)
              i32.const 7
              local.get 0
              br_if 0
              i32.const 1
              i32.add)))
        "#,
        "run",
        vec![WasmValue::I32(0)],
    )
    .await;

    assert_success_i32(result, 8);
}

#[cfg(all(feature = "jit", target_os = "macos", target_arch = "aarch64"))]
#[tokio::test]
async fn jit_br_if_fallback_preserves_taken_stack() {
    let result = invoke_jit(
        r#"
        (module
          (func (export "run") (param i32) (result i32)
            (block (result i32)
              i32.const 7
              local.get 0
              br_if 0
              i32.const 0
              i32.add)))
        "#,
        "run",
        vec![WasmValue::I32(1)],
    )
    .await;

    assert_success_i32(result, 7);
}

#[cfg(all(feature = "jit", target_os = "macos", target_arch = "aarch64"))]
#[tokio::test]
async fn jit_nested_direct_call_fallback_uses_callee_pc_for_fallthrough() {
    let result = invoke_jit(
        r#"
        (module
          (func $branchy (param i32) (result i32)
            (block (result i32)
              i32.const 7
              local.get 0
              br_if 0
              i32.const 1
              i32.add))
          (func (export "run") (param i32) (result i32)
            local.get 0
            call $branchy
            i32.const 2
            i32.add))
        "#,
        "run",
        vec![WasmValue::I32(0)],
    )
    .await;

    assert_success_i32(result, 10);
}

#[cfg(all(feature = "jit", target_os = "macos", target_arch = "aarch64"))]
#[tokio::test]
async fn jit_nested_direct_call_fallback_uses_callee_pc_for_taken_branch() {
    let result = invoke_jit(
        r#"
        (module
          (func $branchy (param i32) (result i32)
            (block (result i32)
              i32.const 7
              local.get 0
              br_if 0
              i32.const 0
              i32.add))
          (func (export "run") (param i32) (result i32)
            local.get 0
            call $branchy
            i32.const 2
            i32.add))
        "#,
        "run",
        vec![WasmValue::I32(1)],
    )
    .await;

    assert_success_i32(result, 9);
}

#[cfg(all(feature = "jit", target_os = "macos", target_arch = "aarch64"))]
#[tokio::test]
async fn jit_executes_i32_memory_load_store_stubs() {
    let result = invoke_jit(
        r#"
        (module
          (memory 1)
          (func $load_store (param i32 i32) (result i32)
            local.get 0
            local.get 1
            i32.store
            local.get 0
            i32.load)
          (func (export "run") (param i32 i32) (result i32)
            local.get 0
            local.get 1
            call $load_store))
        "#,
        "run",
        vec![WasmValue::I32(0), WasmValue::I32(77)],
    )
    .await;

    assert_success_i32(result, 77);
}

#[cfg(all(feature = "jit", target_os = "macos", target_arch = "aarch64"))]
#[tokio::test]
async fn jit_memory_load_traps_on_out_of_bounds() {
    let result = invoke_jit(
        r#"
        (module
          (memory 1)
          (func (export "run") (param i32) (result i32)
            local.get 0
            i32.load))
        "#,
        "run",
        vec![WasmValue::I32(65536)],
    )
    .await;

    assert!(matches!(result, VMResult::MemoryIndexOutOfRange));
}

#[cfg(all(feature = "jit", target_os = "macos", target_arch = "aarch64"))]
#[tokio::test]
async fn jit_returns_unimplemented_when_function_exceeds_code_cache_limit() {
    let module = parse_module(
        r#"
        (module
          (func (export "run") (result i32)
            i32.const 1))
        "#,
    );
    let store = Store::new_with_runtime_config(RuntimeConfig {
        jit: JitConfig {
            enabled: true,
            code_cache_max_bytes: 1,
        },
    });
    let registry = Registry::new();
    let instance = match telomere::instantiate(module, &store, &registry).await {
        VMResult::Success(instance) => instance,
        other => panic!("instantiate must succeed, got {other:?}"),
    };
    let result =
        telomere::run_module_function(&instance, &store, "run", &ResultValue::new(vec![])).await;

    assert!(matches!(result, VMResult::Unimplemented));
}
