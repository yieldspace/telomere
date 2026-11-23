//! Embedder-configured resource limits (#126).
//!
//! One test per limit in [`LimitsConfig`], plus regression checks that the
//! defaults preserve the runtime's previous behaviour. Growth instructions
//! report an exceeded limit by returning -1 (the spec-defined failure
//! surface, identical to exceeding a module-declared maximum); only
//! instantiation failures surface as `VMResult::ResourceLimitExceeded`.

use telomere::{
    instantiate, run_module_function, InstanceHandle, IoReadBinaryReader, Registry, ResultValue,
    RuntimeConfig, Store, VMResult, WasmParser, WasmValue,
};
use wast::parser::ParseBuffer;
use wast::Wat;

fn store_with_limits(edit: impl FnOnce(&mut telomere::LimitsConfig)) -> Store {
    let mut config = RuntimeConfig::default();
    edit(&mut config.limits);
    Store::new_with_runtime_config(config)
}

async fn instantiate_wat_result(
    wat: &str,
    store: &Store,
    registry: &Registry,
) -> VMResult<InstanceHandle> {
    let buf = ParseBuffer::new(wat).unwrap();
    let mut wat_module = wast::parser::parse::<Wat>(&buf).unwrap();
    let source = wat_module.encode().unwrap();
    let mut reader = IoReadBinaryReader::from(&source[..]);
    let mut parser = WasmParser::new(&mut reader);
    let module = parser.parse_module().unwrap();
    instantiate(module, store, registry).await
}

async fn instantiate_wat_ok(wat: &str, store: &Store, registry: &Registry) -> InstanceHandle {
    match instantiate_wat_result(wat, store, registry).await {
        VMResult::Success(instance) => instance,
        other => panic!("expected instantiation to succeed, got {other}"),
    }
}

async fn call_i32(
    instance: &InstanceHandle,
    store: &Store,
    name: &str,
    args: Vec<WasmValue>,
) -> VMResult<i32> {
    match run_module_function(instance, store, name, &ResultValue::new(args)).await {
        VMResult::Success(values) => match values.iter().next() {
            Some(WasmValue::I32(value)) => VMResult::Success(*value),
            other => panic!("expected i32 result from {name}, got {other:?}"),
        },
        VMResult::Unreachable => VMResult::Unreachable,
        VMResult::StackOverflow => VMResult::StackOverflow,
        VMResult::MemoryIndexOutOfRange => VMResult::MemoryIndexOutOfRange,
        VMResult::UnalignedAtomic => VMResult::UnalignedAtomic,
        VMResult::TableIndexOutOfRange => VMResult::TableIndexOutOfRange,
        VMResult::CallIndirectInvalidType => VMResult::CallIndirectInvalidType,
        VMResult::TableUninitialized => VMResult::TableUninitialized,
        VMResult::Unlinkable => VMResult::Unlinkable,
        VMResult::MemoryAllocationFailed => VMResult::MemoryAllocationFailed,
        VMResult::InvalidOperand => VMResult::InvalidOperand,
        VMResult::DivideByZero => VMResult::DivideByZero,
        VMResult::IntegerOverflow => VMResult::IntegerOverflow,
        VMResult::InvalidConversion => VMResult::InvalidConversion,
        VMResult::Unimplemented => VMResult::Unimplemented,
        VMResult::FuelExhausted => VMResult::FuelExhausted,
        VMResult::Cancelled => VMResult::Cancelled,
        VMResult::ResourceLimitExceeded => VMResult::ResourceLimitExceeded,
    }
}

fn assert_success(actual: VMResult<i32>, expected: i32) {
    assert!(
        matches!(actual, VMResult::Success(value) if value == expected),
        "expected Success({expected}), got {actual}"
    );
}

const MEMORY_WAT: &str = r#"
(module
  (memory (export "mem") 1 10)
  (func (export "grow") (param i32) (result i32)
    (memory.grow (local.get 0)))
  (func (export "size") (result i32) (memory.size)))
"#;

const TABLE_WAT: &str = r#"
(module
  (table $t (export "tab") 1 10 funcref)
  (func (export "grow") (param i32) (result i32)
    (table.grow $t (ref.null func) (local.get 0)))
  (func (export "size") (result i32) (table.size $t)))
"#;

const RECURSION_WAT: &str = r#"
(module
  (func $f (export "run") (param i32) (result i32)
    (if (result i32) (i32.eqz (local.get 0))
      (then (i32.const 0))
      (else (call $f (i32.sub (local.get 0) (i32.const 1)))))))
"#;

#[test]
fn default_limits_match_previous_behaviour_values() {
    let config = RuntimeConfig::default();
    assert_eq!(config.limits.max_total_memory_pages, None);
    assert_eq!(config.limits.max_table_elements, None);
    assert_eq!(config.limits.value_stack_bytes, 128 * 1024);
    assert_eq!(config.limits.max_code_cache_bytes, 4 * 1024 * 1024);
}

#[tokio::test]
async fn defaults_impose_no_new_limits() {
    let store = Store::new();
    let registry = Registry::new();

    let memory = instantiate_wat_ok(MEMORY_WAT, &store, &registry).await;
    // Grow within the module-declared maximum works exactly as before.
    assert_success(
        call_i32(&memory, &store, "grow", vec![WasmValue::I32(3)]).await,
        1,
    );
    assert_success(call_i32(&memory, &store, "size", vec![]).await, 4);
    // The module-declared maximum still bounds growth with -1.
    assert_success(
        call_i32(&memory, &store, "grow", vec![WasmValue::I32(100)]).await,
        -1,
    );

    let table = instantiate_wat_ok(TABLE_WAT, &store, &registry).await;
    assert_success(
        call_i32(&table, &store, "grow", vec![WasmValue::I32(4)]).await,
        1,
    );
    assert_success(call_i32(&table, &store, "size", vec![]).await, 5);
}

#[tokio::test]
async fn total_memory_pages_cap_blocks_instantiation() {
    let store = store_with_limits(|limits| limits.max_total_memory_pages = Some(3));
    let registry = Registry::new();

    // A single module whose minimum alone exceeds the cap is refused with
    // the distinguishable limit error.
    let too_big = instantiate_wat_result("(module (memory 4))", &store, &registry).await;
    assert!(
        matches!(too_big, VMResult::ResourceLimitExceeded),
        "expected ResourceLimitExceeded, got {too_big}"
    );

    // Two pages fit; a second instance pushing the store total past the
    // cap is refused even though it would fit on its own.
    instantiate_wat_ok("(module (memory 2))", &store, &registry).await;
    let second = instantiate_wat_result("(module (memory 2))", &store, &registry).await;
    assert!(
        matches!(second, VMResult::ResourceLimitExceeded),
        "expected ResourceLimitExceeded, got {second}"
    );
}

#[tokio::test]
async fn total_memory_pages_cap_blocks_growth_with_minus_one() {
    let store = store_with_limits(|limits| limits.max_total_memory_pages = Some(3));
    let registry = Registry::new();
    let memory = instantiate_wat_ok(MEMORY_WAT, &store, &registry).await;

    // Growing up to the store-wide cap succeeds (previous size returned).
    assert_success(
        call_i32(&memory, &store, "grow", vec![WasmValue::I32(2)]).await,
        1,
    );
    assert_success(call_i32(&memory, &store, "size", vec![]).await, 3);
    // One more page would exceed the cap: -1, not a trap, and no growth.
    assert_success(
        call_i32(&memory, &store, "grow", vec![WasmValue::I32(1)]).await,
        -1,
    );
    assert_success(call_i32(&memory, &store, "size", vec![]).await, 3);
}

#[tokio::test]
async fn per_memory_page_cap_from_memory_config_still_applies() {
    let mut config = RuntimeConfig::default();
    config.memory.max_memory_pages = 1;
    let store = Store::new_with_runtime_config(config);
    let registry = Registry::new();
    let memory = instantiate_wat_ok(MEMORY_WAT, &store, &registry).await;

    // The #125 per-memory ceiling composes with the module maximum: the
    // memory starts at its 1-page minimum and cannot grow past the cap.
    assert_success(
        call_i32(&memory, &store, "grow", vec![WasmValue::I32(1)]).await,
        -1,
    );
    assert_success(call_i32(&memory, &store, "size", vec![]).await, 1);
}

#[tokio::test]
async fn table_element_cap_blocks_instantiation_and_growth() {
    let store = store_with_limits(|limits| limits.max_table_elements = Some(2));
    let registry = Registry::new();

    // A declared minimum above the cap is refused at instantiation.
    let too_big = instantiate_wat_result("(module (table 3 funcref))", &store, &registry).await;
    assert!(
        matches!(too_big, VMResult::ResourceLimitExceeded),
        "expected ResourceLimitExceeded, got {too_big}"
    );

    // A table declared with a larger maximum is clamped to the cap:
    // growth succeeds up to the cap and then returns -1.
    let table = instantiate_wat_ok(TABLE_WAT, &store, &registry).await;
    assert_success(
        call_i32(&table, &store, "grow", vec![WasmValue::I32(1)]).await,
        1,
    );
    assert_success(call_i32(&table, &store, "size", vec![]).await, 2);
    assert_success(
        call_i32(&table, &store, "grow", vec![WasmValue::I32(1)]).await,
        -1,
    );
}

/// Largest power-of-two recursion depth known to succeed on this store's
/// stack, found by exponential probing.
async fn max_successful_depth(instance: &InstanceHandle, store: &Store) -> i32 {
    let mut depth: i32 = 1;
    loop {
        match call_i32(instance, store, "run", vec![WasmValue::I32(depth)]).await {
            VMResult::Success(_) => {
                depth *= 2;
                assert!(depth < (1 << 24), "recursion probe did not overflow");
            }
            VMResult::StackOverflow => return depth / 2,
            other => panic!("unexpected recursion result: {other}"),
        }
    }
}

#[tokio::test]
async fn value_stack_bytes_bounds_recursion_depth() {
    let registry = Registry::new();

    let default_store = Store::new();
    let default_instance = instantiate_wat_ok(RECURSION_WAT, &default_store, &registry).await;
    let baseline_depth = max_successful_depth(&default_instance, &default_store).await;
    assert!(
        baseline_depth >= 64,
        "default stack should allow useful recursion, probed {baseline_depth}"
    );

    // A much smaller stack overflows at the baseline depth but still runs
    // shallow calls.
    let small_store = store_with_limits(|limits| limits.value_stack_bytes = 4096);
    let small_instance = instantiate_wat_ok(RECURSION_WAT, &small_store, &registry).await;
    let overflowed = call_i32(
        &small_instance,
        &small_store,
        "run",
        vec![WasmValue::I32(baseline_depth)],
    )
    .await;
    assert!(
        matches!(overflowed, VMResult::StackOverflow),
        "expected StackOverflow, got {overflowed}"
    );
    assert_success(
        call_i32(
            &small_instance,
            &small_store,
            "run",
            vec![WasmValue::I32(2)],
        )
        .await,
        0,
    );

    // A larger stack admits recursion well beyond the baseline depth.
    let large_store = store_with_limits(|limits| limits.value_stack_bytes = 16 * 128 * 1024);
    let large_instance = instantiate_wat_ok(RECURSION_WAT, &large_store, &registry).await;
    assert_success(
        call_i32(
            &large_instance,
            &large_store,
            "run",
            vec![WasmValue::I32(baseline_depth * 4)],
        )
        .await,
        0,
    );
}

#[test]
fn code_cache_limit_is_configured_through_limits() {
    // The JIT code-cache ceiling lives in the unified limits shape, with
    // the historical 4 MiB default; enforcement itself is covered by the
    // `jit` test suite (`jit_falls_back_when_function_exceeds_code_cache_limit`).
    let store = store_with_limits(|limits| limits.max_code_cache_bytes = 12345);
    assert_eq!(store.runtime_config().limits.max_code_cache_bytes, 12345);
    assert_eq!(
        RuntimeConfig::default().limits.max_code_cache_bytes,
        4 * 1024 * 1024
    );
}

#[test]
fn resource_limit_error_is_distinguishable() {
    let limit = VMResult::<()>::ResourceLimitExceeded;
    assert_eq!(limit.to_string(), "resource limit exceeded");
    assert!(matches!(limit, VMResult::ResourceLimitExceeded));
    assert!(!matches!(limit, VMResult::Unlinkable));
    assert_ne!(limit.to_string(), VMResult::<()>::Unlinkable.to_string());
}
