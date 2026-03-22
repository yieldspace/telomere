mod common;

use common::instantiate_wat;
use telomere::{run_module_function, Registry, ResultValue, Store, VMResult, WasmValue};

async fn call_i32(
    instance: &telomere::common::InstanceHandle,
    store: &Store,
    name: &str,
    args: Vec<WasmValue>,
) -> VMResult<i32> {
    match run_module_function(instance, store, name, &ResultValue::new(args)).await {
        VMResult::Success(values) => match values.iter().next() {
            Some(WasmValue::I32(value)) => VMResult::Success(*value),
            other => panic!("expected i32 result from {name}, got {other:?}"),
        },
        other => vm_result_map_unit(other),
    }
}

#[cfg(feature = "simd")]
async fn call_v128(
    instance: &telomere::common::InstanceHandle,
    store: &Store,
    name: &str,
    args: Vec<WasmValue>,
) -> VMResult<u128> {
    match run_module_function(instance, store, name, &ResultValue::new(args)).await {
        VMResult::Success(values) => match values.iter().next() {
            Some(WasmValue::V128(value)) => VMResult::Success(*value),
            other => panic!("expected v128 result from {name}, got {other:?}"),
        },
        other => match vm_result_map_unit(other) {
            VMResult::Success(_) => unreachable!(),
            VMResult::Unreachable => VMResult::Unreachable,
            VMResult::StackOverflow => VMResult::StackOverflow,
            VMResult::MemoryIndexOutOfRange => VMResult::MemoryIndexOutOfRange,
            VMResult::TableIndexOutOfRange => VMResult::TableIndexOutOfRange,
            VMResult::CallIndirectInvalidType => VMResult::CallIndirectInvalidType,
            VMResult::TableUninitialized => VMResult::TableUninitialized,
            VMResult::Unlinkable => VMResult::Unlinkable,
            VMResult::InvalidOperand => VMResult::InvalidOperand,
            VMResult::UnalignedAtomic => VMResult::UnalignedAtomic,
        },
    }
}

fn vm_result_map_unit(result: VMResult<ResultValue>) -> VMResult<i32> {
    match result {
        VMResult::Success(_) => unreachable!(),
        VMResult::Unreachable => VMResult::Unreachable,
        VMResult::StackOverflow => VMResult::StackOverflow,
        VMResult::MemoryIndexOutOfRange => VMResult::MemoryIndexOutOfRange,
        VMResult::TableIndexOutOfRange => VMResult::TableIndexOutOfRange,
        VMResult::CallIndirectInvalidType => VMResult::CallIndirectInvalidType,
        VMResult::TableUninitialized => VMResult::TableUninitialized,
        VMResult::Unlinkable => VMResult::Unlinkable,
        VMResult::InvalidOperand => VMResult::InvalidOperand,
        VMResult::UnalignedAtomic => VMResult::UnalignedAtomic,
    }
}

fn assert_success_i32(result: VMResult<i32>, expected: i32) {
    match result {
        VMResult::Success(actual) => assert_eq!(actual, expected),
        other => panic!("expected Success({expected}), got {other:?}"),
    }
}

fn assert_memory_oob(result: VMResult<ResultValue>) {
    assert!(
        matches!(result, VMResult::MemoryIndexOutOfRange),
        "expected MemoryIndexOutOfRange, got {result:?}"
    );
}

#[tokio::test]
async fn load_store_follow_wasm_little_endian() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (func (export "store") (param i32)
            i32.const 0
            local.get 0
            i32.store)
          (func (export "load") (result i32)
            i32.const 0
            i32.load)
          (func (export "byte_at") (param i32) (result i32)
            local.get 0
            i32.load8_u))
        "#,
        &store,
        &registry,
    )
    .await;

    assert!(matches!(
        run_module_function(
            &instance,
            &store,
            "store",
            &ResultValue::new(vec![WasmValue::I32(0x1234_5678)])
        )
        .await,
        VMResult::Success(_)
    ));

    assert_success_i32(
        call_i32(&instance, &store, "load", vec![]).await,
        0x1234_5678,
    );
    assert_success_i32(
        call_i32(&instance, &store, "byte_at", vec![WasmValue::I32(0)]).await,
        0x78,
    );
    assert_success_i32(
        call_i32(&instance, &store, "byte_at", vec![WasmValue::I32(1)]).await,
        0x56,
    );
    assert_success_i32(
        call_i32(&instance, &store, "byte_at", vec![WasmValue::I32(2)]).await,
        0x34,
    );
    assert_success_i32(
        call_i32(&instance, &store, "byte_at", vec![WasmValue::I32(3)]).await,
        0x12,
    );
}

#[tokio::test]
async fn const_address_load_store_superinstructions_preserve_semantics() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (func (export "store") (param i32)
            i32.const 8
            local.get 0
            i32.store)
          (func (export "load") (result i32)
            i32.const 8
            i32.load)
          (func (export "byte_at") (param i32) (result i32)
            local.get 0
            i32.load8_u))
        "#,
        &store,
        &registry,
    )
    .await;

    assert!(matches!(
        run_module_function(
            &instance,
            &store,
            "store",
            &ResultValue::new(vec![WasmValue::I32(0x7856_3412)]),
        )
        .await,
        VMResult::Success(_)
    ));
    assert_success_i32(
        call_i32(&instance, &store, "load", vec![]).await,
        0x7856_3412,
    );
    assert_success_i32(
        call_i32(&instance, &store, "byte_at", vec![WasmValue::I32(8)]).await,
        0x12,
    );
    assert_success_i32(
        call_i32(&instance, &store, "byte_at", vec![WasmValue::I32(11)]).await,
        0x78,
    );
}

#[tokio::test]
async fn const_address_load_store_preserve_oob_and_overflow_traps() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (func (export "const_load_oob") (result i32)
            i32.const 65536
            i32.load)
          (func (export "dynamic_load_oob") (param i32) (result i32)
            local.get 0
            i32.load)
          (func (export "const_store_oob") (param i32)
            i32.const 65536
            local.get 0
            i32.store)
          (func (export "dynamic_store_oob") (param i32 i32)
            local.get 0
            local.get 1
            i32.store)
          (func (export "const_load_overflow") (result i32)
            i32.const -1
            i32.load offset=1)
          (func (export "dynamic_load_overflow") (param i32) (result i32)
            local.get 0
            i32.load offset=1)
          (func (export "const_store_overflow") (param i32)
            i32.const -1
            local.get 0
            i32.store offset=1)
          (func (export "dynamic_store_overflow") (param i32 i32)
            local.get 0
            local.get 1
            i32.store offset=1))
        "#,
        &store,
        &registry,
    )
    .await;

    for (name, args) in [
        ("const_load_oob", ResultValue::new(vec![])),
        (
            "dynamic_load_oob",
            ResultValue::new(vec![WasmValue::I32(65536)]),
        ),
        ("const_store_oob", ResultValue::new(vec![WasmValue::I32(1)])),
        (
            "dynamic_store_oob",
            ResultValue::new(vec![WasmValue::I32(65536), WasmValue::I32(1)]),
        ),
        ("const_load_overflow", ResultValue::new(vec![])),
        (
            "dynamic_load_overflow",
            ResultValue::new(vec![WasmValue::I32(-1)]),
        ),
        (
            "const_store_overflow",
            ResultValue::new(vec![WasmValue::I32(1)]),
        ),
        (
            "dynamic_store_overflow",
            ResultValue::new(vec![WasmValue::I32(-1), WasmValue::I32(1)]),
        ),
    ] {
        assert!(
            matches!(
                run_module_function(&instance, &store, name, &args).await,
                VMResult::MemoryIndexOutOfRange
            ),
            "{name} must trap with MemoryIndexOutOfRange"
        );
    }
}

#[tokio::test]
async fn mem_fill_trap_leaves_memory_unchanged() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (func (export "seed")
            i32.const 0xff00
            i32.const 0x11
            i32.store8
            i32.const 0xffff
            i32.const 0x22
            i32.store8)
          (func (export "fill") (param i32 i32 i32)
            local.get 0
            local.get 1
            local.get 2
            memory.fill)
          (func (export "byte_at") (param i32) (result i32)
            local.get 0
            i32.load8_u))
        "#,
        &store,
        &registry,
    )
    .await;

    assert!(matches!(
        run_module_function(&instance, &store, "seed", &ResultValue::new(vec![])).await,
        VMResult::Success(_)
    ));
    assert_memory_oob(
        run_module_function(
            &instance,
            &store,
            "fill",
            &ResultValue::new(vec![
                WasmValue::I32(0xff00),
                WasmValue::I32(0xaa),
                WasmValue::I32(0x101),
            ]),
        )
        .await,
    );
    assert_success_i32(
        call_i32(&instance, &store, "byte_at", vec![WasmValue::I32(0xff00)]).await,
        0x11,
    );
    assert_success_i32(
        call_i32(&instance, &store, "byte_at", vec![WasmValue::I32(0xffff)]).await,
        0x22,
    );
}

#[tokio::test]
async fn mem_copy_and_init_traps_leave_memory_unchanged() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data $payload "AB")
          (func (export "seed")
            i32.const 0
            i32.const 0x33
            i32.store8
            i32.const 1
            i32.const 0x44
            i32.store8
            i32.const 0xffff
            i32.const 0x55
            i32.store8)
          (func (export "copy") (param i32 i32 i32)
            local.get 0
            local.get 1
            local.get 2
            memory.copy)
          (func (export "init") (param i32 i32 i32)
            local.get 0
            local.get 1
            local.get 2
            memory.init $payload)
          (func (export "byte_at") (param i32) (result i32)
            local.get 0
            i32.load8_u))
        "#,
        &store,
        &registry,
    )
    .await;

    assert!(matches!(
        run_module_function(&instance, &store, "seed", &ResultValue::new(vec![])).await,
        VMResult::Success(_)
    ));
    assert_memory_oob(
        run_module_function(
            &instance,
            &store,
            "copy",
            &ResultValue::new(vec![
                WasmValue::I32(0xffff),
                WasmValue::I32(0),
                WasmValue::I32(2),
            ]),
        )
        .await,
    );
    assert_success_i32(
        call_i32(&instance, &store, "byte_at", vec![WasmValue::I32(0xffff)]).await,
        0x55,
    );

    assert_memory_oob(
        run_module_function(
            &instance,
            &store,
            "init",
            &ResultValue::new(vec![
                WasmValue::I32(0xffff),
                WasmValue::I32(0),
                WasmValue::I32(2),
            ]),
        )
        .await,
    );
    assert_success_i32(
        call_i32(&instance, &store, "byte_at", vec![WasmValue::I32(0xffff)]).await,
        0x55,
    );
}

#[tokio::test]
async fn mem_copy_overlap_matches_memmove_semantics() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (func (export "seed")
            i32.const 0
            i32.const 0x01
            i32.store8
            i32.const 1
            i32.const 0x02
            i32.store8
            i32.const 2
            i32.const 0x03
            i32.store8
            i32.const 3
            i32.const 0x04
            i32.store8)
          (func (export "copy")
            i32.const 1
            i32.const 0
            i32.const 3
            memory.copy)
          (func (export "byte_at") (param i32) (result i32)
            local.get 0
            i32.load8_u))
        "#,
        &store,
        &registry,
    )
    .await;

    assert!(matches!(
        run_module_function(&instance, &store, "seed", &ResultValue::new(vec![])).await,
        VMResult::Success(_)
    ));
    assert!(matches!(
        run_module_function(&instance, &store, "copy", &ResultValue::new(vec![])).await,
        VMResult::Success(_)
    ));

    assert_success_i32(
        call_i32(&instance, &store, "byte_at", vec![WasmValue::I32(0)]).await,
        0x01,
    );
    assert_success_i32(
        call_i32(&instance, &store, "byte_at", vec![WasmValue::I32(1)]).await,
        0x01,
    );
    assert_success_i32(
        call_i32(&instance, &store, "byte_at", vec![WasmValue::I32(2)]).await,
        0x02,
    );
    assert_success_i32(
        call_i32(&instance, &store, "byte_at", vec![WasmValue::I32(3)]).await,
        0x03,
    );
}

#[cfg(feature = "threads")]
#[tokio::test]
async fn shared_mem_copy_overlap_matches_memmove_semantics() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1 2 shared)
          (func (export "seed")
            i32.const 0
            i32.const 0x01
            i32.store8
            i32.const 1
            i32.const 0x02
            i32.store8
            i32.const 2
            i32.const 0x03
            i32.store8
            i32.const 3
            i32.const 0x04
            i32.store8)
          (func (export "copy")
            i32.const 1
            i32.const 0
            i32.const 3
            memory.copy)
          (func (export "byte_at") (param i32) (result i32)
            local.get 0
            i32.load8_u))
        "#,
        &store,
        &registry,
    )
    .await;

    assert!(matches!(
        run_module_function(&instance, &store, "seed", &ResultValue::new(vec![])).await,
        VMResult::Success(_)
    ));
    assert!(matches!(
        run_module_function(&instance, &store, "copy", &ResultValue::new(vec![])).await,
        VMResult::Success(_)
    ));

    assert_success_i32(
        call_i32(&instance, &store, "byte_at", vec![WasmValue::I32(0)]).await,
        0x01,
    );
    assert_success_i32(
        call_i32(&instance, &store, "byte_at", vec![WasmValue::I32(1)]).await,
        0x01,
    );
    assert_success_i32(
        call_i32(&instance, &store, "byte_at", vec![WasmValue::I32(2)]).await,
        0x02,
    );
    assert_success_i32(
        call_i32(&instance, &store, "byte_at", vec![WasmValue::I32(3)]).await,
        0x03,
    );
}

#[cfg(feature = "simd")]
async fn assert_simd_memory_roundtrip(memory_decl: &str) {
    let store = Store::new();
    let registry = Registry::new();
    let module = format!(
        r#"
        (module
          (memory {memory_decl})
          (func (export "seed")
            i32.const 0
            i32.const 0x11223344
            i32.store)
          (func (export "load_zero") (result v128)
            i32.const 0
            v128.load32_zero)
          (func (export "store_lane")
            i32.const 4
            v128.const i32x4 0x11223344 0 0 0
            v128.store8_lane 0)
          (func (export "byte_at") (param i32) (result i32)
            local.get 0
            i32.load8_u))
        "#
    );
    let instance = instantiate_wat(&module, &store, &registry).await;

    assert!(matches!(
        run_module_function(&instance, &store, "seed", &ResultValue::new(vec![])).await,
        VMResult::Success(_)
    ));
    let value = match call_v128(&instance, &store, "load_zero", vec![]).await {
        VMResult::Success(value) => value,
        other => panic!("expected simd load success, got {other:?}"),
    };
    assert_eq!(value.to_le_bytes()[0..4], [0x44, 0x33, 0x22, 0x11]);
    assert_eq!(value.to_le_bytes()[4..16], [0; 12]);

    assert!(matches!(
        run_module_function(&instance, &store, "store_lane", &ResultValue::new(vec![])).await,
        VMResult::Success(_)
    ));
    assert_success_i32(
        call_i32(&instance, &store, "byte_at", vec![WasmValue::I32(4)]).await,
        0x44,
    );
}

#[cfg(feature = "simd")]
#[tokio::test]
async fn simd_memory_access_roundtrips_for_local_default_memory() {
    assert_simd_memory_roundtrip("1").await;
}

#[cfg(all(feature = "simd", feature = "threads"))]
#[tokio::test]
async fn simd_memory_access_roundtrips_for_shared_default_memory() {
    assert_simd_memory_roundtrip("1 2 shared").await;
}

#[tokio::test]
async fn memory_grow_returns_previous_page_count_and_minus_one_on_limit() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1 2)
          (func (export "grow") (param i32) (result i32)
            local.get 0
            memory.grow)
          (func (export "size") (result i32)
            memory.size))
        "#,
        &store,
        &registry,
    )
    .await;

    assert_success_i32(call_i32(&instance, &store, "size", vec![]).await, 1);
    assert_success_i32(
        call_i32(&instance, &store, "grow", vec![WasmValue::I32(1)]).await,
        1,
    );
    assert_success_i32(call_i32(&instance, &store, "size", vec![]).await, 2);
    assert_success_i32(
        call_i32(&instance, &store, "grow", vec![WasmValue::I32(1)]).await,
        -1,
    );
}

async fn assert_memory_grow_overflow_returns_minus_one(memory_decl: &str) {
    let store = Store::new();
    let registry = Registry::new();
    let module = format!(
        r#"
        (module
          (memory {memory_decl})
          (func (export "grow") (param i32) (result i32)
            local.get 0
            memory.grow)
          (func (export "size") (result i32)
            memory.size))
        "#
    );
    let instance = instantiate_wat(&module, &store, &registry).await;

    assert_success_i32(call_i32(&instance, &store, "size", vec![]).await, 1);
    assert_success_i32(
        call_i32(&instance, &store, "grow", vec![WasmValue::I32(-1)]).await,
        -1,
    );
    assert_success_i32(call_i32(&instance, &store, "size", vec![]).await, 1);
}

#[tokio::test]
async fn memory_grow_overflow_keeps_local_memory_size_unchanged() {
    assert_memory_grow_overflow_returns_minus_one("1 10").await;
}

#[cfg(feature = "threads")]
#[tokio::test]
async fn memory_grow_overflow_keeps_shared_memory_size_unchanged() {
    assert_memory_grow_overflow_returns_minus_one("1 10 shared").await;
}

#[tokio::test]
async fn indexed_local_memory_ops_support_nonzero_memidx() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (memory $dst 1 2)
          (data $payload "\44\33\22\11")
          (func (export "init_dst")
            (memory.init $dst $payload (i32.const 8) (i32.const 0) (i32.const 4)))
          (func (export "load_dst") (result i32)
            (i32.load $dst (i32.const 8)))
          (func (export "fill_dst")
            (memory.fill $dst (i32.const 12) (i32.const 0xaa) (i32.const 4)))
          (func (export "byte_dst") (param i32) (result i32)
            (i32.load8_u $dst (local.get 0)))
          (func (export "size_dst") (result i32)
            (memory.size $dst))
          (func (export "grow_dst") (param i32) (result i32)
            (memory.grow $dst (local.get 0))))
        "#,
        &store,
        &registry,
    )
    .await;

    let store_result =
        run_module_function(&instance, &store, "init_dst", &ResultValue::new(vec![])).await;
    assert!(
        matches!(store_result, VMResult::Success(_)),
        "store result: {store_result:?}"
    );
    assert_success_i32(call_i32(&instance, &store, "size_dst", vec![]).await, 1);
    assert_success_i32(
        call_i32(&instance, &store, "grow_dst", vec![WasmValue::I32(1)]).await,
        1,
    );
    assert_success_i32(call_i32(&instance, &store, "size_dst", vec![]).await, 2);
    assert_success_i32(
        call_i32(&instance, &store, "load_dst", vec![]).await,
        0x1122_3344,
    );
    assert!(matches!(
        run_module_function(&instance, &store, "fill_dst", &ResultValue::new(vec![])).await,
        VMResult::Success(_)
    ));
    assert_success_i32(
        call_i32(&instance, &store, "byte_dst", vec![WasmValue::I32(12)]).await,
        0xaa,
    );
}

#[cfg(feature = "threads")]
#[tokio::test]
async fn indexed_cross_memory_copy_supports_local_to_shared() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (memory $src 1)
          (memory $dst 1 2 shared)
          (func (export "seed_src")
            (i32.store $src (i32.const 0) (i32.const 0x55667788)))
          (func (export "copy_to_dst")
            (memory.copy $dst $src (i32.const 12) (i32.const 0) (i32.const 4)))
          (func (export "load_dst") (result i32)
            (i32.load $dst (i32.const 12))))
        "#,
        &store,
        &registry,
    )
    .await;

    assert!(matches!(
        run_module_function(&instance, &store, "seed_src", &ResultValue::new(vec![])).await,
        VMResult::Success(_)
    ));
    assert!(matches!(
        run_module_function(&instance, &store, "copy_to_dst", &ResultValue::new(vec![])).await,
        VMResult::Success(_)
    ));
    assert_success_i32(
        call_i32(&instance, &store, "load_dst", vec![]).await,
        0x5566_7788,
    );
}

#[cfg(feature = "simd")]
async fn assert_indexed_simd_memory_roundtrip(memory_decl: &str) {
    let store = Store::new();
    let registry = Registry::new();
    let module = format!(
        r#"
        (module
          (memory 1)
          (memory $m {memory_decl})
          (func (export "seed")
            (i32.store $m (i32.const 0) (i32.const 0x11223344)))
          (func (export "load_zero") (result v128)
            (v128.load32_zero $m (i32.const 0)))
          (func (export "store_lane")
            (v128.store8_lane $m 0 (i32.const 4) (v128.const i32x4 0x11223344 0 0 0)))
          (func (export "byte_at") (param i32) (result i32)
            (i32.load8_u $m (local.get 0))))
        "#
    );
    let instance = instantiate_wat(&module, &store, &registry).await;

    assert!(matches!(
        run_module_function(&instance, &store, "seed", &ResultValue::new(vec![])).await,
        VMResult::Success(_)
    ));
    let value = match call_v128(&instance, &store, "load_zero", vec![]).await {
        VMResult::Success(value) => value,
        other => panic!("expected simd load success, got {other:?}"),
    };
    assert_eq!(value.to_le_bytes()[0..4], [0x44, 0x33, 0x22, 0x11]);
    assert_eq!(value.to_le_bytes()[4..16], [0; 12]);

    assert!(matches!(
        run_module_function(&instance, &store, "store_lane", &ResultValue::new(vec![])).await,
        VMResult::Success(_)
    ));
    assert_success_i32(
        call_i32(&instance, &store, "byte_at", vec![WasmValue::I32(4)]).await,
        0x44,
    );
}

#[cfg(feature = "simd")]
#[tokio::test]
async fn indexed_simd_memory_access_roundtrips_for_local_nonzero_memidx() {
    assert_indexed_simd_memory_roundtrip("1").await;
}

#[cfg(all(feature = "threads", feature = "simd"))]
#[tokio::test]
async fn indexed_simd_memory_access_roundtrips_for_shared_nonzero_memidx() {
    assert_indexed_simd_memory_roundtrip("1 2 shared").await;
}
