#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
use telomere::{
    common::{ExecuteContext, InstanceHandle, Instr},
    link_host_function_with_function_idx, Registry, ResultValue, VMResult, WasmValue,
};
use telomere::{JitConfig, RuntimeConfig, Store};

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
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

#[test]
fn jit_supported_matches_target_matrix() {
    let expected = cfg!(all(
        feature = "jit",
        any(
            all(target_os = "macos", target_arch = "aarch64"),
            all(
                any(target_os = "macos", target_os = "linux"),
                target_arch = "x86_64"
            ),
            all(
                target_os = "linux",
                target_arch = "riscv64",
                target_env = "gnu"
            )
        )
    ));
    assert_eq!(telomere::jit_supported(), expected);
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
fn jit_store() -> Store {
    Store::new_with_runtime_config(RuntimeConfig {
        jit: JitConfig {
            enabled: true,
            ..JitConfig::default()
        },
    })
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
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

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
async fn instantiate_jit_wat(wat: &str, store: &Store, registry: &Registry) -> InstanceHandle {
    let module = parse_module(wat);
    match telomere::instantiate(module, store, registry).await {
        VMResult::Success(instance) => instance,
        other => panic!("instantiate must succeed, got {other:?}"),
    }
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
fn host_add_one(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    let value = i32::from_le_bytes(
        ctx.stack
            .local_bytes(&ctx.local_reference(), 0, 4)
            .try_into()
            .unwrap(),
    );
    ctx.return_slot().write(&(value + 1).to_le_bytes());
    let (prev_local_ref, return_addr) =
        ctx.stack
            .function_return_in_place(&ctx.local_reference, 4, ctx.gc);
    ctx.set_local_reference(prev_local_ref);
    VMResult::Success(return_addr)
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
fn host_add_ten(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    let value = i32::from_le_bytes(
        ctx.stack
            .local_bytes(&ctx.local_reference(), 0, 4)
            .try_into()
            .unwrap(),
    );
    ctx.return_slot().write(&(value + 10).to_le_bytes());
    let (prev_local_ref, return_addr) =
        ctx.stack
            .function_return_in_place(&ctx.local_reference, 4, ctx.gc);
    ctx.set_local_reference(prev_local_ref);
    VMResult::Success(return_addr)
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
fn assert_success_i32(result: VMResult<ResultValue>, expected: i32) {
    assert_success_values(result, ResultValue::new(vec![WasmValue::I32(expected)]));
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
fn assert_success_values(result: VMResult<ResultValue>, expected: ResultValue) {
    let VMResult::Success(values) = result else {
        panic!("expected success {expected:?}, got {result:?}");
    };
    assert_eq!(values, expected);
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
fn assert_jit_accepted(
    before: telomere::runtime::jit::JitCacheStats,
    after: telomere::runtime::jit::JitCacheStats,
) {
    assert!(
        after.compiled_functions > before.compiled_functions,
        "expected JIT cache to accept a function, before={before:?} after={after:?}"
    );
    assert_eq!(
        after.rejected_functions, before.rejected_functions,
        "expected no JIT compile rejection, before={before:?} after={after:?}"
    );
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
trait VmResultMap<T> {
    fn map<U>(self, f: impl FnOnce(T) -> U) -> VMResult<U>;
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
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

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
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

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_signed_remainder_overflow_returns_zero() {
    let result = invoke_jit(
        r#"
        (module
          (func (export "run") (result i32 i64)
            i32.const -2147483648
            i32.const -1
            i32.rem_s
            i64.const -9223372036854775808
            i64.const -1
            i64.rem_s))
        "#,
        "run",
        vec![],
    )
    .await;

    assert_success_values(
        result,
        ResultValue::new(vec![WasmValue::I32(0), WasmValue::I64(0)]),
    );
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
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

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
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

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_repeated_lazy_direct_call_keeps_cache_state_stable() {
    let store = jit_store();
    let registry = Registry::new();
    let instance = instantiate_jit_wat(
        r#"
        (module
          (func $add1 (param i32) (result i32)
            local.get 0
            i32.const 1
            i32.add)
          (func (export "run") (param i32) (result i32)
            local.get 0
            call $add1
            call $add1
            i32.const 3
            i32.add))
        "#,
        &store,
        &registry,
    )
    .await;

    let before = store.jit_cache_stats();
    let first = telomere::run_module_function(
        &instance,
        &store,
        "run",
        &ResultValue::new(vec![WasmValue::I32(10)]),
    )
    .await;
    let after_first = store.jit_cache_stats();
    let second = telomere::run_module_function(
        &instance,
        &store,
        "run",
        &ResultValue::new(vec![WasmValue::I32(20)]),
    )
    .await;
    let after_second = store.jit_cache_stats();

    assert_success_i32(first, 15);
    assert_success_i32(second, 25);
    assert_jit_accepted(before, after_first);
    assert_eq!(
        after_second.compiled_functions, after_first.compiled_functions,
        "expected repeated lazy direct calls to reuse accepted JIT entries, first={after_first:?} second={after_second:?}"
    );
    assert_eq!(
        after_second.rejected_functions, after_first.rejected_functions,
        "expected no JIT rejection on repeated lazy direct calls, first={after_first:?} second={after_second:?}"
    );
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_wasm_fast_direct_call_falls_back_after_host_relink() {
    let store = jit_store();
    let registry = Registry::new();
    let instance = instantiate_jit_wat(
        r#"
        (module
          (func $target (param i32) (result i32)
            local.get 0
            i32.const 1
            i32.add)
          (func (export "run") (param i32) (result i32)
            local.get 0
            call $target
            i32.const 2
            i32.add))
        "#,
        &store,
        &registry,
    )
    .await;

    let before = store.jit_cache_stats();
    let first = telomere::run_module_function(
        &instance,
        &store,
        "run",
        &ResultValue::new(vec![WasmValue::I32(10)]),
    )
    .await;
    let after_first = store.jit_cache_stats();
    link_host_function_with_function_idx(&instance, 0, host_add_ten, &store);
    let second = telomere::run_module_function(
        &instance,
        &store,
        "run",
        &ResultValue::new(vec![WasmValue::I32(10)]),
    )
    .await;
    let after_second = store.jit_cache_stats();

    assert_success_i32(first, 13);
    assert_success_i32(second, 22);
    assert_jit_accepted(before, after_first);
    assert_eq!(
        after_second.rejected_functions, after_first.rejected_functions,
        "relink fallback should not reject the already-compiled caller, first={after_first:?} second={after_second:?}"
    );
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_call_to_rejected_callee_resumes_native_caller() {
    let result = invoke_jit(
        r#"
        (module
          (table 1 funcref)
          (func $callee (result i32)
            table.size)
          (func (export "run") (result i32)
            i32.const 10
            call $callee
            i32.add))
        "#,
        "run",
        Vec::new(),
    )
    .await;

    assert_success_i32(result, 11);
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
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

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
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

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_i32_const_cmp_br_if_block_result_taken() {
    let result = invoke_jit(
        r#"
        (module
          (func (export "run") (param i32) (result i32)
            (block (result i32)
              i32.const 7
              local.get 0
              i32.const 65536
              i32.lt_u
              br_if 0
              drop
              i32.const 9)))
        "#,
        "run",
        vec![WasmValue::I32(4)],
    )
    .await;

    assert_success_i32(result, 7);
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_i32_const_cmp_br_if_block_result_fallthrough() {
    let result = invoke_jit(
        r#"
        (module
          (func (export "run") (param i32) (result i32)
            (block (result i32)
              i32.const 7
              local.get 0
              i32.const 65536
              i32.lt_u
              br_if 0
              drop
              i32.const 9)))
        "#,
        "run",
        vec![WasmValue::I32(65536)],
    )
    .await;

    assert_success_i32(result, 9);
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_nested_i32_const_cmp_br_if_block_result_taken() {
    let result = invoke_jit(
        r#"
        (module
          (func $branchy (param i32) (result i32)
            (block (result i32)
              i32.const 7
              local.get 0
              i32.const 65536
              i32.lt_u
              br_if 0
              drop
              i32.const 9))
          (func (export "run") (param i32) (result i32)
            local.get 0
            call $branchy
            i32.const 2
            i32.add))
        "#,
        "run",
        vec![WasmValue::I32(4)],
    )
    .await;

    assert_success_i32(result, 9);
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_repeated_i32_const_cmp_br_if_block_result_taken_does_not_leak_stack() {
    let result = invoke_jit(
        r#"
        (module
          (func $branchy (param i32) (result i32)
            (block (result i32)
              i32.const 7
              local.get 0
              i32.const 65536
              i32.lt_u
              br_if 0
              drop
              i32.const 9))
          (func (export "run") (param i32) (result i32)
            (local i32 i32)
            i32.const 0
            local.set 1
            i32.const 0
            local.set 2
            loop
              local.get 2
              local.get 0
              i32.ge_u
              if
                local.get 1
                return
              end
              local.get 1
              local.get 2
              call $branchy
              i32.add
              local.set 1
              local.get 2
              i32.const 1
              i32.add
              local.set 2
              br 0
            end
            unreachable))
        "#,
        "run",
        vec![WasmValue::I32(1000)],
    )
    .await;

    assert_success_i32(result, 7000);
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
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

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
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

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_import_direct_call_uses_runtime_helper() {
    let store = jit_store();
    let mut registry = Registry::new();
    let host = instantiate_jit_wat(
        r#"
        (module
          (func (export "add_one") (param i32) (result i32)
            local.get 0))
        "#,
        &store,
        &registry,
    )
    .await;
    link_host_function_with_function_idx(&host, 0, host_add_one, &store);
    registry.register("host", host);
    let instance = instantiate_jit_wat(
        r#"
        (module
          (import "host" "add_one" (func $add_one (param i32) (result i32)))
          (func (export "run") (param i32) (result i32)
            local.get 0
            call $add_one
            i32.const 2
            i32.add))
        "#,
        &store,
        &registry,
    )
    .await;
    let result = telomere::run_module_function(
        &instance,
        &store,
        "run",
        &ResultValue::new(vec![WasmValue::I32(41)]),
    )
    .await;

    assert_success_i32(result, 44);
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_exported_host_function_uses_host_start_trampoline() {
    let store = jit_store();
    let mut registry = Registry::new();
    let host = instantiate_jit_wat(
        r#"
        (module
          (func (export "add_one") (param i32) (result i32)
            local.get 0))
        "#,
        &store,
        &registry,
    )
    .await;
    link_host_function_with_function_idx(&host, 0, host_add_one, &store);
    registry.register("host", host);
    let instance = instantiate_jit_wat(
        r#"
        (module
          (import "host" "add_one" (func $add_one (param i32) (result i32)))
          (export "run" (func $add_one)))
        "#,
        &store,
        &registry,
    )
    .await;
    let result = telomere::run_module_function(
        &instance,
        &store,
        "run",
        &ResultValue::new(vec![WasmValue::I32(41)]),
    )
    .await;

    assert_success_i32(result, 42);
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_import_return_call_uses_runtime_helper() {
    let store = jit_store();
    let mut registry = Registry::new();
    let host = instantiate_jit_wat(
        r#"
        (module
          (func (export "add_one") (param i32) (result i32)
            local.get 0))
        "#,
        &store,
        &registry,
    )
    .await;
    link_host_function_with_function_idx(&host, 0, host_add_one, &store);
    registry.register("host", host);
    let instance = instantiate_jit_wat(
        r#"
        (module
          (import "host" "add_one" (func $add_one (param i32) (result i32)))
          (func (export "run") (param i32) (result i32)
            local.get 0
            return_call $add_one))
        "#,
        &store,
        &registry,
    )
    .await;
    let result = telomere::run_module_function(
        &instance,
        &store,
        "run",
        &ResultValue::new(vec![WasmValue::I32(41)]),
    )
    .await;

    assert_success_i32(result, 42);
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_runtime_handler_executes_unsupported_i64_ops() {
    let result = invoke_jit(
        r#"
        (module
          (func (export "run") (param i64) (result i64)
            local.get 0
            i64.const 1
            i64.add))
        "#,
        "run",
        vec![WasmValue::I64(41)],
    )
    .await;

    assert_success_values(result, ResultValue::new(vec![WasmValue::I64(42)]));
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_inlines_global_get_set_for_scalar_simd_and_refs() {
    let result = invoke_jit(
        r#"
        (module
          (func $target)
          (elem declare funcref (ref.func $target))
          (global $i32 (mut i32) (i32.const 0))
          (global $i64 (mut i64) (i64.const 0))
          (global $f32 (mut f32) (f32.const 0))
          (global $f64 (mut f64) (f64.const 0))
          (global $v128 (mut v128) (v128.const i32x4 0 0 0 0))
          (global $funcref (mut funcref) (ref.null func))
          (global $externref (mut externref) (ref.null extern))
          (func (export "run") (result i32 i64 f32 f64 v128 i32 i32)
            i32.const 123456
            global.set $i32
            global.get $i32

            i64.const 0x1122334455667788
            global.set $i64
            global.get $i64

            f32.const 3.5
            global.set $f32
            global.get $f32

            f64.const -9.25
            global.set $f64
            global.get $f64

            v128.const i8x16 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15
            global.set $v128
            global.get $v128

            ref.func $target
            global.set $funcref
            global.get $funcref
            ref.is_null

            ref.null extern
            global.set $externref
            global.get $externref
            ref.is_null))
        "#,
        "run",
        Vec::new(),
    )
    .await;

    assert_success_values(
        result,
        ResultValue::new(vec![
            WasmValue::I32(123456),
            WasmValue::I64(0x1122334455667788),
            WasmValue::F32(3.5),
            WasmValue::F64(-9.25),
            WasmValue::V128(u128::from_le_bytes([
                0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
            ])),
            WasmValue::I32(0),
            WasmValue::I32(1),
        ]),
    );
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_inlines_imported_mutable_global_access() {
    let store = jit_store();
    let mut registry = Registry::new();
    let producer = instantiate_jit_wat(
        r#"
        (module
          (global (export "g") (mut i32) (i32.const 10)))
        "#,
        &store,
        &registry,
    )
    .await;
    registry.register("producer", producer);
    let consumer = instantiate_jit_wat(
        r#"
        (module
          (import "producer" "g" (global $g (mut i32)))
          (func (export "run") (param i32) (result i32)
            local.get 0
            global.set $g
            global.get $g))
        "#,
        &store,
        &registry,
    )
    .await;

    let result = telomere::run_module_function(
        &consumer,
        &store,
        "run",
        &ResultValue::new(vec![WasmValue::I32(77)]),
    )
    .await;

    assert_success_i32(result, 77);
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_runtime_handler_flushes_native_stack_before_fallback() {
    let result = invoke_jit(
        r#"
        (module
          (func (export "run") (result i32)
            i32.const 7
            drop
            i32.const 9))
        "#,
        "run",
        Vec::new(),
    )
    .await;

    assert_success_i32(result, 9);
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_runtime_handler_handles_direct_emit_resource_limits() {
    let result = invoke_jit(
        r#"
        (module
          (func (export "run") (result i32)
            i32.const 1
            i32.const 2
            i32.const 3
            i32.const 4
            i32.const 5
            i32.const 6
            i32.const 7
            i32.const 8
            i32.add
            i32.add
            i32.add
            i32.add
            i32.add
            i32.add
            i32.add))
        "#,
        "run",
        Vec::new(),
    )
    .await;

    assert_success_i32(result, 36);
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_executes_i32_bitwise_and_variable_shifts() {
    let result = invoke_jit(
        r#"
        (module
          (func (export "run") (param $x i32) (param $y i32) (result i32)
            local.get $x
            local.get $y
            i32.and
            local.get $x
            local.get $y
            i32.or
            i32.add
            local.get $x
            local.get $y
            i32.xor
            i32.add
            local.get $x
            i32.const 1
            i32.shl
            i32.add
            local.get $x
            i32.const 1
            i32.shr_u
            i32.add
            local.get $x
            i32.const 1
            i32.shr_s
            i32.add
            local.get $x
            i32.const 1
            i32.rotl
            i32.add
            local.get $x
            i32.const 1
            i32.rotr
            i32.add))
        "#,
        "run",
        vec![WasmValue::I32(10), WasmValue::I32(12)],
    )
    .await;

    assert_success_i32(result, 83);
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_executes_native_popcnt_and_i64_unary_ops() {
    let result = invoke_jit(
        r#"
        (module
          (func (export "run") (result i64)
            i64.const 255
            i64.clz
            i64.const 255
            i64.ctz
            i64.add
            i64.const 255
            i64.popcnt
            i64.add
            i32.const 0xf0f0f0f0
            i32.popcnt
            i64.extend_i32_u
            i64.add))
        "#,
        "run",
        Vec::new(),
    )
    .await;

    assert_success_values(result, ResultValue::new(vec![WasmValue::I64(80)]));
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_executes_native_float_rounding_ops() {
    let result = invoke_jit(
        r#"
        (module
          (func (export "run") (param f32) (param f64) (result f32 f64)
            local.get 0
            f32.ceil
            local.get 0
            f32.floor
            f32.add
            local.get 0
            f32.trunc
            f32.add
            local.get 0
            f32.nearest
            f32.add

            local.get 1
            f64.ceil
            local.get 1
            f64.floor
            f64.add
            local.get 1
            f64.trunc
            f64.add
            local.get 1
            f64.nearest
            f64.add))
        "#,
        "run",
        vec![WasmValue::F32(1.75), WasmValue::F64(-1.5)],
    )
    .await;

    assert_success_values(
        result,
        ResultValue::new(vec![WasmValue::F32(6.0), WasmValue::F64(-6.0)]),
    );
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_executes_native_float_helpers_and_conversions() {
    let result = invoke_jit(
        r#"
        (module
          (func (export "run") (result f32 f64 i32 i64 f32 f32 f32 f32 f32 f32 f64)
            f32.const -0.0
            f32.const 0.0
            f32.min
            f64.const -4.0
            f64.const 2.0
            f64.copysign
            f32.const 42.9
            i32.trunc_f32_s
            f64.const 123.9
            i64.trunc_f64_u
            i32.const -7
            f32.convert_i32_s
            i32.const 7
            f32.convert_i32_u
            f32.const 3.25
            f32.const -0.0
            f32.copysign
            i64.const -9
            f32.convert_i64_s
            i64.const 11
            f32.convert_i64_u
            f64.const 6.75
            f32.demote_f64
            f32.const 1.5
            f64.promote_f32))
        "#,
        "run",
        Vec::new(),
    )
    .await;

    assert_success_values(
        result,
        ResultValue::new(vec![
            WasmValue::F32(-0.0),
            WasmValue::F64(4.0),
            WasmValue::I32(42),
            WasmValue::I64(123),
            WasmValue::F32(-7.0),
            WasmValue::F32(7.0),
            WasmValue::F32(-3.25),
            WasmValue::F32(-9.0),
            WasmValue::F32(11.0),
            WasmValue::F32(6.75),
            WasmValue::F64(1.5),
        ]),
    );
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_float_conversion_edge_cases_match_wasm_semantics() {
    let store = jit_store();
    let registry = Registry::new();
    let instance = instantiate_jit_wat(
        r#"
        (module
          (func (export "run") (result f32 f64 i32 i32 i32 i32)
            i64.const -1
            f32.convert_i64_u
            i64.const -1
            f64.convert_i64_u
            f32.const -1.0
            i32.trunc_sat_f32_u
            f32.const inf
            i32.trunc_sat_f32_s
            f64.const 4294967296.0
            i32.trunc_sat_f64_u
            f64.const nan
            i32.trunc_sat_f64_s))
        "#,
        &store,
        &registry,
    )
    .await;

    let before = store.jit_cache_stats();
    let result =
        telomere::run_module_function(&instance, &store, "run", &ResultValue::new(vec![])).await;
    let after = store.jit_cache_stats();

    assert_success_values(
        result,
        ResultValue::new(vec![
            WasmValue::F32(u64::MAX as f32),
            WasmValue::F64(u64::MAX as f64),
            WasmValue::I32(0),
            WasmValue::I32(i32::MAX),
            WasmValue::I32(-1),
            WasmValue::I32(0),
        ]),
    );
    assert_jit_accepted(before, after);
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_executes_i32_compare_family() {
    let result = invoke_jit(
        r#"
        (module
          (func (export "run") (param $a i32) (param $b i32) (result i32)
            local.get $a
            local.get $b
            i32.eq
            i32.const 1
            i32.mul
            local.get $a
            local.get $b
            i32.ne
            i32.const 2
            i32.mul
            i32.add
            local.get $a
            local.get $b
            i32.lt_s
            i32.const 4
            i32.mul
            i32.add
            local.get $a
            local.get $b
            i32.lt_u
            i32.const 8
            i32.mul
            i32.add
            local.get $a
            local.get $b
            i32.gt_s
            i32.const 16
            i32.mul
            i32.add
            local.get $a
            local.get $b
            i32.gt_u
            i32.const 32
            i32.mul
            i32.add
            local.get $a
            local.get $b
            i32.le_s
            i32.const 64
            i32.mul
            i32.add
            local.get $a
            local.get $b
            i32.le_u
            i32.const 128
            i32.mul
            i32.add
            local.get $a
            local.get $b
            i32.ge_s
            i32.const 256
            i32.mul
            i32.add
            local.get $a
            local.get $b
            i32.ge_u
            i32.const 512
            i32.mul
            i32.add))
        "#,
        "run",
        vec![WasmValue::I32(5), WasmValue::I32(7)],
    )
    .await;

    assert_success_i32(result, 206);
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_executes_local_get_br_table() {
    let wat = r#"
        (module
          (func (export "run") (param $x i32) (result i32)
            (block $default
              (block $case1
                (block $case0
                  local.get $x
                  br_table $case0 $case1 $default)
                i32.const 10
                return)
              i32.const 20
              return)
            i32.const 30))
        "#;

    assert_success_i32(invoke_jit(wat, "run", vec![WasmValue::I32(0)]).await, 10);
    assert_success_i32(invoke_jit(wat, "run", vec![WasmValue::I32(1)]).await, 20);
    assert_success_i32(invoke_jit(wat, "run", vec![WasmValue::I32(2)]).await, 30);
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_executes_local_get_const_add_br_table() {
    let wat = r#"
        (module
          (func (export "run") (param $x i32) (result i32)
            (block $default
              (block $case1
                (block $case0
                  local.get $x
                  i32.const 1
                  i32.add
                  br_table $case0 $case1 $default)
                i32.const 10
                return)
              i32.const 20
              return)
            i32.const 30))
        "#;

    assert_success_i32(invoke_jit(wat, "run", vec![WasmValue::I32(-1)]).await, 10);
    assert_success_i32(invoke_jit(wat, "run", vec![WasmValue::I32(0)]).await, 20);
    assert_success_i32(invoke_jit(wat, "run", vec![WasmValue::I32(1)]).await, 30);
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
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

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_executes_native_wide_memory_ops() {
    let result = invoke_jit(
        r#"
        (module
          (memory 1)
          (func (export "run") (result i64 i32 f32 f64 i32)
            i32.const 0
            i64.const 0x1122334455667788
            i64.store
            i32.const 0
            i64.load

            i32.const 8
            i64.const 0x123456789abc
            i64.store16
            i32.const 8
            i32.load16_u

            i32.const 16
            f32.const 3.5
            f32.store
            i32.const 16
            f32.load

            i32.const 24
            f64.const -9.25
            f64.store
            i32.const 24
            f64.load

            memory.size))
        "#,
        "run",
        Vec::new(),
    )
    .await;

    assert_success_values(
        result,
        ResultValue::new(vec![
            WasmValue::I64(0x1122334455667788),
            WasmValue::I32(0x9abc),
            WasmValue::F32(3.5),
            WasmValue::F64(-9.25),
            WasmValue::I32(1),
        ]),
    );
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
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

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_i64_store_trap_does_not_partially_write() {
    let store = jit_store();
    let registry = Registry::new();
    let instance = instantiate_jit_wat(
        r#"
        (module
          (memory 1)
          (func (export "store") (param i32 i64)
            local.get 0
            local.get 1
            i64.store align=4)
          (func (export "load") (param i32) (result i32)
            local.get 0
            i32.load))
        "#,
        &store,
        &registry,
    )
    .await;

    let before = store.jit_cache_stats();
    let store_result = telomere::run_module_function(
        &instance,
        &store,
        "store",
        &ResultValue::new(vec![WasmValue::I32(65532), WasmValue::I64(-1)]),
    )
    .await;
    let after_store = store.jit_cache_stats();
    let load_result = telomere::run_module_function(
        &instance,
        &store,
        "load",
        &ResultValue::new(vec![WasmValue::I32(65532)]),
    )
    .await;
    let after_load = store.jit_cache_stats();

    assert!(matches!(store_result, VMResult::MemoryIndexOutOfRange));
    assert_success_i32(load_result, 0);
    assert_jit_accepted(before, after_store);
    assert_jit_accepted(after_store, after_load);
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_br_if_preserves_vm_stack_value_after_continuation_bridge() {
    let store = jit_store();
    let registry = Registry::new();
    let instance = instantiate_jit_wat(
        r#"
        (module
          (func (export "run") (param i32) (result i32)
            (block (result i32)
              v128.const i8x16 7 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
              i8x16.extract_lane_s 0
              local.get 0
              br_if 0
              drop
              i32.const 11)))
        "#,
        &store,
        &registry,
    )
    .await;

    let before = store.jit_cache_stats();
    let fallthrough = telomere::run_module_function(
        &instance,
        &store,
        "run",
        &ResultValue::new(vec![WasmValue::I32(0)]),
    )
    .await;
    let after_fallthrough = store.jit_cache_stats();
    let taken = telomere::run_module_function(
        &instance,
        &store,
        "run",
        &ResultValue::new(vec![WasmValue::I32(1)]),
    )
    .await;
    let after_taken = store.jit_cache_stats();

    assert_success_i32(fallthrough, 11);
    assert_success_i32(taken, 7);
    assert_jit_accepted(before, after_fallthrough);
    assert_eq!(
        after_taken.rejected_functions, after_fallthrough.rejected_functions,
        "expected no JIT compile rejection, before={after_fallthrough:?} after={after_taken:?}"
    );
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_nested_br_if_value_preserves_fallthrough_branch_value() {
    let result = invoke_jit(
        r#"
        (module
          (func (export "run") (param i32) (result i32)
            (i32.add
              (i32.const 1)
              (block (result i32)
                (drop (i32.const 2))
                (drop (br_if 0
                  (block (result i32)
                    (drop (br_if 1 (i32.const 8) (local.get 0)))
                    (i32.const 4))
                  (i32.const 1)))
                (i32.const 16)))))
        "#,
        "run",
        vec![WasmValue::I32(0)],
    )
    .await;

    assert_success_i32(result, 5);
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_native_multi_value_control_branches() {
    let store = jit_store();
    let registry = Registry::new();
    let instance = instantiate_jit_wat(
        r#"
        (module
          (func (export "br_multi_base") (result i32)
            i32.const 5
            block (result i32 i32 i64)
              i32.const 7
              i32.const 11
              i64.const 13
              br 0
            end
            drop
            i32.add
            i32.add)

          (func (export "br_if_multi") (param $take i32) (result i32 i64)
            block (result i32 i64)
              i32.const 4
              i64.const 9
              local.get $take
              br_if 0
              drop
              drop
              i32.const 5
              i64.const 10
            end)

          (func (export "br_table_multi") (param $idx i32) (result i32 i32)
            block $outer (result i32 i32)
              block $inner (result i32 i32)
                i32.const 3
                i32.const 4
                local.get $idx
                br_table $inner $outer
              end
              i32.const 10
              i32.add
            end)

          (func (export "loop_params") (param $n i32) (result i32 i32)
            (local $a i32)
            (local $b i32)
            block $exit (result i32 i32)
              i32.const 1
              i32.const 10
              loop $loop (param i32 i32)
                local.set $b
                local.set $a
                local.get $n
                i32.eqz
                if
                  local.get $a
                  local.get $b
                  br $exit
                end
                local.get $n
                i32.const 1
                i32.sub
                local.set $n
                local.get $a
                i32.const 1
                i32.add
                local.get $b
                i32.const 2
                i32.add
                br $loop
              end
              unreachable
            end))
        "#,
        &store,
        &registry,
    )
    .await;

    let mut before = store.jit_cache_stats();
    let br_multi = telomere::run_module_function(
        &instance,
        &store,
        "br_multi_base",
        &ResultValue::new(vec![]),
    )
    .await;
    let mut after = store.jit_cache_stats();
    assert_success_i32(br_multi, 23);
    assert_jit_accepted(before, after);

    before = after;
    let br_if_taken = telomere::run_module_function(
        &instance,
        &store,
        "br_if_multi",
        &ResultValue::new(vec![WasmValue::I32(1)]),
    )
    .await;
    after = store.jit_cache_stats();
    assert_success_values(
        br_if_taken,
        ResultValue::new(vec![WasmValue::I32(4), WasmValue::I64(9)]),
    );
    assert_jit_accepted(before, after);

    let after_taken = after;
    let br_if_fallthrough = telomere::run_module_function(
        &instance,
        &store,
        "br_if_multi",
        &ResultValue::new(vec![WasmValue::I32(0)]),
    )
    .await;
    after = store.jit_cache_stats();
    assert_success_values(
        br_if_fallthrough,
        ResultValue::new(vec![WasmValue::I32(5), WasmValue::I64(10)]),
    );
    assert_eq!(after.compiled_functions, after_taken.compiled_functions);
    assert_eq!(after.rejected_functions, after_taken.rejected_functions);

    before = after;
    let br_table_inner = telomere::run_module_function(
        &instance,
        &store,
        "br_table_multi",
        &ResultValue::new(vec![WasmValue::I32(0)]),
    )
    .await;
    after = store.jit_cache_stats();
    assert_success_values(
        br_table_inner,
        ResultValue::new(vec![WasmValue::I32(3), WasmValue::I32(14)]),
    );
    assert_jit_accepted(before, after);

    let after_inner = after;
    let br_table_outer = telomere::run_module_function(
        &instance,
        &store,
        "br_table_multi",
        &ResultValue::new(vec![WasmValue::I32(1)]),
    )
    .await;
    after = store.jit_cache_stats();
    assert_success_values(
        br_table_outer,
        ResultValue::new(vec![WasmValue::I32(3), WasmValue::I32(4)]),
    );
    assert_eq!(after.compiled_functions, after_inner.compiled_functions);
    assert_eq!(after.rejected_functions, after_inner.rejected_functions);

    before = after;
    let loop_params = telomere::run_module_function(
        &instance,
        &store,
        "loop_params",
        &ResultValue::new(vec![WasmValue::I32(3)]),
    )
    .await;
    after = store.jit_cache_stats();
    assert_success_values(
        loop_params,
        ResultValue::new(vec![WasmValue::I32(4), WasmValue::I32(16)]),
    );
    assert_jit_accepted(before, after);
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_executes_bulk_memory_runtime_stubs() {
    let result = invoke_jit(
        r#"
        (module
          (memory 1)
          (data $d "abcd")
          (func (export "run") (result i32)
            i32.const 0
            i32.const 0
            i32.const 4
            memory.init $d
            data.drop $d
            i32.const 0
            i32.load))
        "#,
        "run",
        Vec::new(),
    )
    .await;

    assert_success_i32(result, 0x64636261);
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_executes_table_runtime_stubs() {
    let result = invoke_jit(
        r#"
        (module
          (table 1 funcref)
          (func (export "run") (result i32)
            i32.const 0
            table.get 0
            ref.is_null))
        "#,
        "run",
        Vec::new(),
    )
    .await;

    assert_success_i32(result, 1);
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_executes_simd_runtime_stubs() {
    let result = invoke_jit(
        r#"
        (module
          (func (export "run") (result i32)
            v128.const i8x16 255 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15
            i8x16.extract_lane_s 0))
        "#,
        "run",
        Vec::new(),
    )
    .await;

    assert_success_i32(result, -1);
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_executes_atomic_runtime_stubs() {
    let result = invoke_jit(
        r#"
        (module
          (memory 1 1 shared)
          (func (export "run") (result i32)
            i32.const 0
            i32.const 7
            i32.atomic.store
            i32.const 0
            i32.const 8
            i64.const 0
            memory.atomic.wait32
            i32.const 0
            i32.const 1
            memory.atomic.notify
            i32.add))
        "#,
        "run",
        Vec::new(),
    )
    .await;

    assert_success_i32(result, 1);
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_accepts_runtime_stub_without_compile_reject() {
    let store = jit_store();
    let registry = Registry::new();
    let instance = instantiate_jit_wat(
        r#"
        (module
          (memory 1)
          (data $d "abcd")
          (func (export "run") (result i32)
            i32.const 0
            i32.const 0
            i32.const 4
            memory.init $d
            data.drop $d
            i32.const 0
            i32.load))
        "#,
        &store,
        &registry,
    )
    .await;

    let before = store.jit_cache_stats();
    let result =
        telomere::run_module_function(&instance, &store, "run", &ResultValue::new(vec![])).await;
    let after = store.jit_cache_stats();

    assert_success_i32(result, 0x64636261);
    assert_jit_accepted(before, after);
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_accepts_runtime_continuation_stub_without_compile_reject() {
    let store = jit_store();
    let registry = Registry::new();
    let instance = instantiate_jit_wat(
        r#"
        (module
          (func (export "run") (result i32)
            v128.const i8x16 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16
            v128.const i8x16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32
            i8x16.shuffle 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31
            i8x16.extract_lane_s 0))
        "#,
        &store,
        &registry,
    )
    .await;

    let before = store.jit_cache_stats();
    let result =
        telomere::run_module_function(&instance, &store, "run", &ResultValue::new(vec![])).await;
    let after = store.jit_cache_stats();

    assert_success_i32(result, 17);
    assert_jit_accepted(before, after);
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_accepts_current_op_continuation_bridge_without_compile_reject() {
    let store = jit_store();
    let registry = Registry::new();
    let instance = instantiate_jit_wat(
        r#"
        (module
          (memory 1 1 shared)
          (func (export "run") (result i32 i32)
            i32.const 0
            i32.const 5
            i32.atomic.store
            i32.const 0
            i32.const 3
            i32.atomic.rmw.add
            i32.const 0
            i32.atomic.load))
        "#,
        &store,
        &registry,
    )
    .await;

    let before = store.jit_cache_stats();
    let result =
        telomere::run_module_function(&instance, &store, "run", &ResultValue::new(vec![])).await;
    let after = store.jit_cache_stats();

    assert_success_values(
        result,
        ResultValue::new(vec![WasmValue::I32(5), WasmValue::I32(8)]),
    );
    assert_jit_accepted(before, after);
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_falls_back_when_function_exceeds_code_cache_limit() {
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

    assert_success_i32(result, 1);
}
