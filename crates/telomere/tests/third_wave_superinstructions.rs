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
        other => match other {
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

async fn call_i64(
    instance: &telomere::common::InstanceHandle,
    store: &Store,
    name: &str,
    args: Vec<WasmValue>,
) -> VMResult<i64> {
    match run_module_function(instance, store, name, &ResultValue::new(args)).await {
        VMResult::Success(values) => match values.iter().next() {
            Some(WasmValue::I64(value)) => VMResult::Success(*value),
            other => panic!("expected i64 result from {name}, got {other:?}"),
        },
        other => match other {
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

async fn call_f32_bits(
    instance: &telomere::common::InstanceHandle,
    store: &Store,
    name: &str,
    args: Vec<WasmValue>,
) -> VMResult<u32> {
    match run_module_function(instance, store, name, &ResultValue::new(args)).await {
        VMResult::Success(values) => match values.iter().next() {
            Some(WasmValue::F32(value)) => VMResult::Success(value.to_bits()),
            other => panic!("expected f32 result from {name}, got {other:?}"),
        },
        other => match other {
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

async fn call_f64_bits(
    instance: &telomere::common::InstanceHandle,
    store: &Store,
    name: &str,
    args: Vec<WasmValue>,
) -> VMResult<u64> {
    match run_module_function(instance, store, name, &ResultValue::new(args)).await {
        VMResult::Success(values) => match values.iter().next() {
            Some(WasmValue::F64(value)) => VMResult::Success(value.to_bits()),
            other => panic!("expected f64 result from {name}, got {other:?}"),
        },
        other => match other {
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

fn assert_invalid_operand<T: std::fmt::Debug>(result: VMResult<T>, name: &str) {
    assert!(
        matches!(result, VMResult::InvalidOperand),
        "{name} must trap with InvalidOperand, got {result:?}"
    );
}

fn assert_memory_oob<T: std::fmt::Debug>(result: VMResult<T>, name: &str) {
    assert!(
        matches!(result, VMResult::MemoryIndexOutOfRange),
        "{name} must trap with MemoryIndexOutOfRange, got {result:?}"
    );
}

fn assert_success<T: PartialEq + std::fmt::Debug>(result: VMResult<T>, expected: T, name: &str) {
    match result {
        VMResult::Success(actual) => assert_eq!(actual, expected, "{name} returned wrong value"),
        other => panic!("expected Success({expected:?}) from {name}, got {other:?}"),
    }
}

#[tokio::test]
async fn typed_scalar_superinstructions_match_unfused_semantics() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "fused_i32_xor") (param i32 i32) (result i32)
            local.get 0
            local.get 1
            i32.xor
            local.set 0
            local.get 0)
          (func (export "baseline_i32_xor") (param i32 i32) (result i32)
            local.get 0
            local.get 1
            i32.xor
            i32.const 0
            i32.add
            local.set 0
            local.get 0)
          (func (export "fused_i64_add") (param i64) (result i64)
            local.get 0
            i64.const 5
            i64.add
            local.set 0
            local.get 0)
          (func (export "baseline_i64_add") (param i64) (result i64)
            local.get 0
            i64.const 5
            i64.add
            i64.const 0
            i64.add
            local.set 0
            local.get 0)
          (func (export "fused_f32_mul") (param f32) (result f32)
            local.get 0
            f32.const 1.5
            f32.mul
            local.set 0
            local.get 0)
          (func (export "baseline_f32_mul") (param f32) (result f32)
            local.get 0
            f32.const 1.5
            f32.mul
            f32.const 0
            f32.add
            local.set 0
            local.get 0)
          (func (export "fused_f64_div_tee") (param f64 f64) (result f64)
            local.get 0
            local.get 1
            f64.div
            local.tee 0)
          (func (export "baseline_f64_div_tee") (param f64 f64) (result f64)
            local.get 0
            local.get 1
            f64.div
            f64.const 0
            f64.add
            local.tee 0))
        "#,
        &store,
        &registry,
    )
    .await;

    let i32_args = vec![WasmValue::I32(0x55aa_00ff), WasmValue::I32(0x0f0f_f000)];
    let expected_i32 = 0x55aa_00ff_i32 ^ 0x0f0f_f000_i32;
    assert_success(
        call_i32(&instance, &store, "fused_i32_xor", i32_args.clone()).await,
        expected_i32,
        "fused_i32_xor",
    );
    assert_success(
        call_i32(&instance, &store, "baseline_i32_xor", i32_args).await,
        expected_i32,
        "baseline_i32_xor",
    );

    assert_success(
        call_i64(&instance, &store, "fused_i64_add", vec![WasmValue::I64(41)]).await,
        46,
        "fused_i64_add",
    );
    assert_success(
        call_i64(
            &instance,
            &store,
            "baseline_i64_add",
            vec![WasmValue::I64(41)],
        )
        .await,
        46,
        "baseline_i64_add",
    );

    assert_success(
        call_f32_bits(
            &instance,
            &store,
            "fused_f32_mul",
            vec![WasmValue::F32(2.0)],
        )
        .await,
        3.0f32.to_bits(),
        "fused_f32_mul",
    );
    assert_success(
        call_f32_bits(
            &instance,
            &store,
            "baseline_f32_mul",
            vec![WasmValue::F32(2.0)],
        )
        .await,
        3.0f32.to_bits(),
        "baseline_f32_mul",
    );

    let f64_args = vec![WasmValue::F64(9.0), WasmValue::F64(2.0)];
    assert_success(
        call_f64_bits(&instance, &store, "fused_f64_div_tee", f64_args.clone()).await,
        4.5f64.to_bits(),
        "fused_f64_div_tee",
    );
    assert_success(
        call_f64_bits(&instance, &store, "baseline_f64_div_tee", f64_args).await,
        4.5f64.to_bits(),
        "baseline_f64_div_tee",
    );
}

#[tokio::test]
async fn producer_only_scalar_superinstructions_match_expected_results() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "i32_add_push") (param i32) (result i32)
            local.get 0
            i32.const 7
            i32.add
            i32.const 3
            i32.xor)
          (func (export "i32_local_local_sub_push") (param i32 i32) (result i32)
            local.get 0
            local.get 1
            i32.sub
            i32.const 1
            i32.add)
          (func (export "i32_div_u_push_trap") (param i32) (result i32)
            local.get 0
            i32.const 0
            i32.div_u
            i32.const 1
            i32.add))
        "#,
        &store,
        &registry,
    )
    .await;

    assert_success(
        call_i32(&instance, &store, "i32_add_push", vec![WasmValue::I32(9)]).await,
        (9i32.wrapping_add(7)) ^ 3,
        "i32_add_push",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "i32_local_local_sub_push",
            vec![WasmValue::I32(20), WasmValue::I32(6)],
        )
        .await,
        15,
        "i32_local_local_sub_push",
    );
    assert_invalid_operand(
        call_i32(
            &instance,
            &store,
            "i32_div_u_push_trap",
            vec![WasmValue::I32(9)],
        )
        .await,
        "i32_div_u_push_trap",
    );
}

#[tokio::test]
async fn i32_local_and_branch_superinstructions_match_expected_results() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "and_eqz_br_if") (param i32) (result i32) (local i32)
            i32.const 9
            local.set 1
            block
              local.get 0
              i32.const 2
              i32.and
              i32.eqz
              br_if 0
              i32.const 7
              local.set 1
            end
            local.get 1)
          (func (export "and_if") (param i32) (result i32)
            local.get 0
            i32.const 1
            i32.and
            if (result i32)
              i32.const 7
            else
              i32.const 3
            end))
        "#,
        &store,
        &registry,
    )
    .await;

    assert_success(
        call_i32(&instance, &store, "and_eqz_br_if", vec![WasmValue::I32(0)]).await,
        9,
        "and_eqz_br_if zero branch",
    );
    assert_success(
        call_i32(&instance, &store, "and_eqz_br_if", vec![WasmValue::I32(2)]).await,
        7,
        "and_eqz_br_if fallthrough",
    );
    assert_success(
        call_i32(&instance, &store, "and_if", vec![WasmValue::I32(1)]).await,
        7,
        "and_if truthy",
    );
    assert_success(
        call_i32(&instance, &store, "and_if", vec![WasmValue::I32(2)]).await,
        3,
        "and_if falsy",
    );
}

#[tokio::test]
async fn local_copy_and_branch_superinstructions_match_expected_results() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "copy_set") (param i32) (result i32)
            (local i32)
            local.get 0
            local.set 1
            local.get 1)
          (func (export "copy_tee") (param i32) (result i32)
            (local i32)
            local.get 0
            local.tee 1
            drop
            local.get 1)
          (func (export "local_br_if") (param i32) (result i32)
            (local i32)
            i32.const 11
            local.set 1
            block
              local.get 0
              br_if 0
              i32.const 7
              local.set 1
            end
            local.get 1)
          (func (export "local_eqz_if") (param i32) (result i32)
            local.get 0
            i32.eqz
            if (result i32)
              i32.const 7
            else
              i32.const 9
            end))
        "#,
        &store,
        &registry,
    )
    .await;

    assert_success(
        call_i32(&instance, &store, "copy_set", vec![WasmValue::I32(1234)]).await,
        1234,
        "copy_set",
    );
    assert_success(
        call_i32(&instance, &store, "copy_tee", vec![WasmValue::I32(5678)]).await,
        5678,
        "copy_tee",
    );
    assert_success(
        call_i32(&instance, &store, "local_br_if", vec![WasmValue::I32(0)]).await,
        7,
        "local_br_if zero",
    );
    assert_success(
        call_i32(&instance, &store, "local_br_if", vec![WasmValue::I32(5)]).await,
        11,
        "local_br_if nonzero",
    );
    assert_success(
        call_i32(&instance, &store, "local_eqz_if", vec![WasmValue::I32(0)]).await,
        7,
        "local_eqz_if zero",
    );
    assert_success(
        call_i32(&instance, &store, "local_eqz_if", vec![WasmValue::I32(5)]).await,
        9,
        "local_eqz_if nonzero",
    );
}

#[tokio::test]
async fn load_mask_branch_superinstructions_match_unfused_semantics_and_traps() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (func (export "seed") (param i32 i32)
            local.get 0
            local.get 1
            i32.store8)
          (func (export "fused_if") (param i32) (result i32)
            (local i32)
            i32.const 9
            local.set 1
            local.get 0
            i32.load8_u
            i32.const 32
            i32.and
            i32.eqz
            if
              i32.const 7
              local.set 1
            end
            local.get 1)
          (func (export "baseline_if") (param i32) (result i32)
            (local i32)
            i32.const 9
            local.set 1
            local.get 0
            i32.load8_u
            i32.const 32
            i32.and
            i32.eqz
            i32.const 0
            i32.or
            if
              i32.const 7
              local.set 1
            end
            local.get 1)
          (func (export "fused_br_if") (param i32) (result i32)
            block $taken
              local.get 0
              i32.load8_u
              i32.const 32
              i32.and
              i32.eqz
              br_if $taken
              i32.const 0
              return
            end
            i32.const 1)
          (func (export "baseline_br_if") (param i32) (result i32)
            block $taken
              local.get 0
              i32.load8_u
              i32.const 32
              i32.and
              i32.eqz
              i32.const 0
              i32.or
              br_if $taken
              i32.const 0
              return
            end
            i32.const 1))
        "#,
        &store,
        &registry,
    )
    .await;

    assert!(matches!(
        run_module_function(
            &instance,
            &store,
            "seed",
            &ResultValue::new(vec![WasmValue::I32(0), WasmValue::I32(0)]),
        )
        .await,
        VMResult::Success(_)
    ));
    assert_success(
        call_i32(&instance, &store, "fused_if", vec![WasmValue::I32(0)]).await,
        7,
        "fused_if zero bit",
    );
    assert_success(
        call_i32(&instance, &store, "baseline_if", vec![WasmValue::I32(0)]).await,
        7,
        "baseline_if zero bit",
    );
    assert_success(
        call_i32(&instance, &store, "fused_br_if", vec![WasmValue::I32(0)]).await,
        1,
        "fused_br_if zero bit",
    );
    assert_success(
        call_i32(&instance, &store, "baseline_br_if", vec![WasmValue::I32(0)]).await,
        1,
        "baseline_br_if zero bit",
    );

    assert!(matches!(
        run_module_function(
            &instance,
            &store,
            "seed",
            &ResultValue::new(vec![WasmValue::I32(0), WasmValue::I32(32)]),
        )
        .await,
        VMResult::Success(_)
    ));
    assert_success(
        call_i32(&instance, &store, "fused_if", vec![WasmValue::I32(0)]).await,
        9,
        "fused_if masked bit",
    );
    assert_success(
        call_i32(&instance, &store, "baseline_if", vec![WasmValue::I32(0)]).await,
        9,
        "baseline_if masked bit",
    );
    assert_success(
        call_i32(&instance, &store, "fused_br_if", vec![WasmValue::I32(0)]).await,
        0,
        "fused_br_if masked bit",
    );
    assert_success(
        call_i32(&instance, &store, "baseline_br_if", vec![WasmValue::I32(0)]).await,
        0,
        "baseline_br_if masked bit",
    );

    assert_memory_oob(
        call_i32(&instance, &store, "fused_if", vec![WasmValue::I32(65536)]).await,
        "fused_if_oob",
    );
    assert_memory_oob(
        call_i32(
            &instance,
            &store,
            "baseline_if",
            vec![WasmValue::I32(65536)],
        )
        .await,
        "baseline_if_oob",
    );
}

#[tokio::test]
async fn load_modify_store_superinstruction_matches_unfused_semantics() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (func (export "seed") (param i32 i32)
            local.get 0
            local.get 1
            i32.store)
          (func (export "read") (param i32) (result i32)
            local.get 0
            i32.load)
          (func (export "fused_update") (param i32) (result i32)
            (local i32)
            local.get 0
            local.get 0
            i32.load
            local.tee 1
            i32.const 4
            i32.add
            i32.store
            local.get 1)
          (func (export "baseline_update") (param i32) (result i32)
            (local i32)
            local.get 0
            local.get 0
            i32.load
            i32.const 0
            i32.or
            local.tee 1
            i32.const 4
            i32.add
            i32.store
            local.get 1))
        "#,
        &store,
        &registry,
    )
    .await;

    assert!(matches!(
        run_module_function(
            &instance,
            &store,
            "seed",
            &ResultValue::new(vec![WasmValue::I32(8), WasmValue::I32(100)]),
        )
        .await,
        VMResult::Success(_)
    ));
    assert_success(
        call_i32(&instance, &store, "fused_update", vec![WasmValue::I32(8)]).await,
        100,
        "fused_update tee result",
    );
    assert_success(
        call_i32(&instance, &store, "read", vec![WasmValue::I32(8)]).await,
        104,
        "fused_update stored result",
    );

    assert!(matches!(
        run_module_function(
            &instance,
            &store,
            "seed",
            &ResultValue::new(vec![WasmValue::I32(12), WasmValue::I32(200)]),
        )
        .await,
        VMResult::Success(_)
    ));
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_update",
            vec![WasmValue::I32(12)],
        )
        .await,
        200,
        "baseline_update tee result",
    );
    assert_success(
        call_i32(&instance, &store, "read", vec![WasmValue::I32(12)]).await,
        204,
        "baseline_update stored result",
    );

    assert_memory_oob(
        call_i32(
            &instance,
            &store,
            "fused_update",
            vec![WasmValue::I32(65536)],
        )
        .await,
        "fused_update_oob",
    );
    assert_memory_oob(
        call_i32(
            &instance,
            &store,
            "baseline_update",
            vec![WasmValue::I32(65536)],
        )
        .await,
        "baseline_update_oob",
    );
}

#[tokio::test]
async fn tee_consumer_superinstructions_match_unfused_semantics_and_traps() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (func (export "seed") (param i32 i32)
            local.get 0
            local.get 1
            i32.store8)
          (func (export "fused_load_if") (param i32) (result i32)
            (local i32)
            block
              local.get 0
              i32.load8_u
              local.tee 1
              i32.eqz
              if
                i32.const 7
                local.set 1
              end
            end
            local.get 1)
          (func (export "baseline_load_if") (param i32) (result i32)
            (local i32)
            block
              local.get 0
              i32.load8_u
              local.tee 1
              i32.const 0
              i32.or
              i32.eqz
              if
                i32.const 7
                local.set 1
              end
            end
            local.get 1)
          (func (export "fused_load_br_if") (param i32) (result i32)
            (local i32)
            block $skip
              local.get 0
              i32.load8_u
              local.tee 1
              i32.eqz
              br_if $skip
              i32.const 7
              local.set 1
            end
            local.get 1)
          (func (export "baseline_load_br_if") (param i32) (result i32)
            (local i32)
            block $skip
              local.get 0
              i32.load8_u
              local.tee 1
              i32.const 0
              i32.or
              i32.eqz
              br_if $skip
              i32.const 7
              local.set 1
            end
            local.get 1)
          (func (export "fused_compare_if") (param i32) (result i32)
            (local i32)
            block
              local.get 0
              local.tee 1
              i32.const 32
              i32.gt_u
              if
                i32.const 7
                local.set 1
              end
            end
            local.get 1)
          (func (export "baseline_compare_if") (param i32) (result i32)
            (local i32)
            block
              local.get 0
              local.tee 1
              i32.const 32
              i32.gt_u
              i32.const 0
              i32.or
              if
                i32.const 7
                local.set 1
              end
            end
            local.get 1)
          (func (export "fused_compare_br_if") (param i32) (result i32)
            (local i32)
            block $skip
              local.get 0
              local.tee 1
              i32.const 32
              i32.gt_u
              br_if $skip
              i32.const 7
              local.set 1
            end
            local.get 1)
          (func (export "baseline_compare_br_if") (param i32) (result i32)
            (local i32)
            block $skip
              local.get 0
              local.tee 1
              i32.const 32
              i32.gt_u
              i32.const 0
              i32.or
              br_if $skip
              i32.const 7
              local.set 1
            end
            local.get 1)
          (func (export "fused_shift_set") (param i32) (result i32)
            (local i32)
            local.get 0
            local.tee 1
            i32.const 3
            i32.shl
            local.set 1
            local.get 1)
          (func (export "baseline_shift_set") (param i32) (result i32)
            (local i32)
            local.get 0
            local.tee 1
            i32.const 3
            i32.shl
            i32.const 0
            i32.or
            local.set 1
            local.get 1)
          (func (export "fused_shift_tee") (param i32) (result i32)
            (local i32)
            local.get 0
            local.tee 1
            i32.const 3
            i32.shl
            local.tee 1)
          (func (export "baseline_shift_tee") (param i32) (result i32)
            (local i32)
            local.get 0
            local.tee 1
            i32.const 3
            i32.shl
            i32.const 0
            i32.or
            local.tee 1)
          (func (export "fused_self_select") (param i32) (result i32)
            (local i32)
            local.get 0
            local.tee 1
            i32.const 7
            local.get 1
            select)
          (func (export "baseline_self_select") (param i32) (result i32)
            (local i32)
            local.get 0
            local.tee 1
            i32.const 0
            i32.or
            i32.const 7
            local.get 1
            select))
        "#,
        &store,
        &registry,
    )
    .await;

    assert!(matches!(
        run_module_function(
            &instance,
            &store,
            "seed",
            &ResultValue::new(vec![WasmValue::I32(0), WasmValue::I32(0)]),
        )
        .await,
        VMResult::Success(_)
    ));
    assert_success(
        call_i32(&instance, &store, "fused_load_if", vec![WasmValue::I32(0)]).await,
        7,
        "fused_load_if zero",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_load_if",
            vec![WasmValue::I32(0)],
        )
        .await,
        7,
        "baseline_load_if zero",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "fused_load_br_if",
            vec![WasmValue::I32(0)],
        )
        .await,
        0,
        "fused_load_br_if zero",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_load_br_if",
            vec![WasmValue::I32(0)],
        )
        .await,
        0,
        "baseline_load_br_if zero",
    );

    assert!(matches!(
        run_module_function(
            &instance,
            &store,
            "seed",
            &ResultValue::new(vec![WasmValue::I32(0), WasmValue::I32(32)]),
        )
        .await,
        VMResult::Success(_)
    ));
    assert_success(
        call_i32(&instance, &store, "fused_load_if", vec![WasmValue::I32(0)]).await,
        32,
        "fused_load_if masked",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_load_if",
            vec![WasmValue::I32(0)],
        )
        .await,
        32,
        "baseline_load_if masked",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "fused_load_br_if",
            vec![WasmValue::I32(0)],
        )
        .await,
        7,
        "fused_load_br_if masked",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_load_br_if",
            vec![WasmValue::I32(0)],
        )
        .await,
        7,
        "baseline_load_br_if masked",
    );

    assert_success(
        call_i32(
            &instance,
            &store,
            "fused_compare_if",
            vec![WasmValue::I32(40)],
        )
        .await,
        7,
        "fused_compare_if true",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_compare_if",
            vec![WasmValue::I32(40)],
        )
        .await,
        7,
        "baseline_compare_if true",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "fused_compare_br_if",
            vec![WasmValue::I32(40)],
        )
        .await,
        40,
        "fused_compare_br_if true",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_compare_br_if",
            vec![WasmValue::I32(40)],
        )
        .await,
        40,
        "baseline_compare_br_if true",
    );

    assert_success(
        call_i32(
            &instance,
            &store,
            "fused_shift_set",
            vec![WasmValue::I32(5)],
        )
        .await,
        40,
        "fused_shift_set",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_shift_set",
            vec![WasmValue::I32(5)],
        )
        .await,
        40,
        "baseline_shift_set",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "fused_shift_tee",
            vec![WasmValue::I32(5)],
        )
        .await,
        40,
        "fused_shift_tee",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_shift_tee",
            vec![WasmValue::I32(5)],
        )
        .await,
        40,
        "baseline_shift_tee",
    );

    assert_success(
        call_i32(
            &instance,
            &store,
            "fused_self_select",
            vec![WasmValue::I32(0)],
        )
        .await,
        7,
        "fused_self_select zero",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_self_select",
            vec![WasmValue::I32(0)],
        )
        .await,
        7,
        "baseline_self_select zero",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "fused_self_select",
            vec![WasmValue::I32(11)],
        )
        .await,
        11,
        "fused_self_select nonzero",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_self_select",
            vec![WasmValue::I32(11)],
        )
        .await,
        11,
        "baseline_self_select nonzero",
    );

    assert_memory_oob(
        call_i32(
            &instance,
            &store,
            "fused_load_if",
            vec![WasmValue::I32(65536)],
        )
        .await,
        "fused_load_if_oob",
    );
    assert_memory_oob(
        call_i32(
            &instance,
            &store,
            "baseline_load_if",
            vec![WasmValue::I32(65536)],
        )
        .await,
        "baseline_load_if_oob",
    );
}

#[tokio::test]
async fn compare_tee_select_superinstructions_match_unfused_semantics() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "fused_compare_select") (param i32 i32 i32 i32) (result i32) (local i32)
            local.get 0
            local.get 1
            local.get 2
            local.get 3
            i32.lt_u
            local.tee 4
            select)
          (func (export "baseline_compare_select") (param i32 i32 i32 i32) (result i32) (local i32)
            local.get 0
            local.get 1
            local.get 2
            local.get 3
            i32.lt_u
            local.tee 4
            i32.const 0
            i32.or
            select))
        "#,
        &store,
        &registry,
    )
    .await;

    assert_success(
        call_i32(
            &instance,
            &store,
            "fused_compare_select",
            vec![
                WasmValue::I32(111),
                WasmValue::I32(222),
                WasmValue::I32(3),
                WasmValue::I32(9),
            ],
        )
        .await,
        111,
        "fused_compare_select true",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_compare_select",
            vec![
                WasmValue::I32(111),
                WasmValue::I32(222),
                WasmValue::I32(3),
                WasmValue::I32(9),
            ],
        )
        .await,
        111,
        "baseline_compare_select true",
    );

    assert_success(
        call_i32(
            &instance,
            &store,
            "fused_compare_select",
            vec![
                WasmValue::I32(111),
                WasmValue::I32(222),
                WasmValue::I32(9),
                WasmValue::I32(3),
            ],
        )
        .await,
        222,
        "fused_compare_select false",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_compare_select",
            vec![
                WasmValue::I32(111),
                WasmValue::I32(222),
                WasmValue::I32(9),
                WasmValue::I32(3),
            ],
        )
        .await,
        222,
        "baseline_compare_select false",
    );
}

#[tokio::test]
async fn const_set_superinstructions_match_expected_results() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "i32_const_set") (result i32)
            (local i32)
            i32.const 7
            local.set 0
            local.get 0)
          (func (export "f64_const_set") (result f64)
            (local f64)
            f64.const 4.5
            local.set 0
            local.get 0))
        "#,
        &store,
        &registry,
    )
    .await;

    assert_success(
        call_i32(&instance, &store, "i32_const_set", vec![]).await,
        7,
        "i32_const_set",
    );
    assert_success(
        call_f64_bits(&instance, &store, "f64_const_set", vec![]).await,
        4.5f64.to_bits(),
        "f64_const_set",
    );
}

#[tokio::test]
async fn typed_select_fast_paths_preserve_wasm_operand_order() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "choose_i32") (param i32 i32 i32) (result i32)
            local.get 0
            local.get 1
            local.get 2
            select)
          (func (export "choose_i64") (param i64 i64 i32) (result i64)
            local.get 0
            local.get 1
            local.get 2
            select))
        "#,
        &store,
        &registry,
    )
    .await;

    assert_success(
        call_i32(
            &instance,
            &store,
            "choose_i32",
            vec![WasmValue::I32(10), WasmValue::I32(20), WasmValue::I32(0)],
        )
        .await,
        20,
        "choose_i32 false branch",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "choose_i32",
            vec![WasmValue::I32(10), WasmValue::I32(20), WasmValue::I32(1)],
        )
        .await,
        10,
        "choose_i32 true branch",
    );
    assert_success(
        call_i64(
            &instance,
            &store,
            "choose_i64",
            vec![WasmValue::I64(11), WasmValue::I64(22), WasmValue::I32(0)],
        )
        .await,
        22,
        "choose_i64 false branch",
    );
    assert_success(
        call_i64(
            &instance,
            &store,
            "choose_i64",
            vec![WasmValue::I64(11), WasmValue::I64(22), WasmValue::I32(1)],
        )
        .await,
        11,
        "choose_i64 true branch",
    );
}

#[tokio::test]
async fn compare_select_superinstructions_match_unfused_semantics() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "fused_i32_lt_select") (param i32 i32 i32 i32) (result i32)
            local.get 0
            local.get 1
            local.get 2
            local.get 3
            i32.lt_u
            select)
          (func (export "baseline_i32_lt_select") (param i32 i32 i32 i32) (result i32) (local i32)
            local.get 2
            local.get 3
            i32.lt_u
            local.set 4
            local.get 0
            local.get 1
            local.get 4
            select)
          (func (export "fused_f64_const_select") (param f64 f64 f64) (result f64)
            local.get 0
            local.get 1
            local.get 2
            f64.const 0
            f64.gt
            select)
          (func (export "baseline_f64_const_select") (param f64 f64 f64) (result f64) (local i32)
            local.get 2
            f64.const 0
            f64.gt
            local.set 3
            local.get 0
            local.get 1
            local.get 3
            select))
        "#,
        &store,
        &registry,
    )
    .await;

    let i32_args = vec![
        WasmValue::I32(111),
        WasmValue::I32(222),
        WasmValue::I32(3),
        WasmValue::I32(9),
    ];
    assert_success(
        call_i32(&instance, &store, "fused_i32_lt_select", i32_args.clone()).await,
        111,
        "fused_i32_lt_select",
    );
    assert_success(
        call_i32(&instance, &store, "baseline_i32_lt_select", i32_args).await,
        111,
        "baseline_i32_lt_select",
    );

    let f64_args = vec![
        WasmValue::F64(1.5),
        WasmValue::F64(9.5),
        WasmValue::F64(-0.25),
    ];
    assert_success(
        call_f64_bits(
            &instance,
            &store,
            "fused_f64_const_select",
            f64_args.clone(),
        )
        .await,
        9.5f64.to_bits(),
        "fused_f64_const_select",
    );
    assert_success(
        call_f64_bits(&instance, &store, "baseline_f64_const_select", f64_args).await,
        9.5f64.to_bits(),
        "baseline_f64_const_select",
    );
}

#[tokio::test]
async fn producer_tee_branch_superinstructions_match_unfused_semantics_and_traps() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (func (export "seed") (param i32 i32)
            local.get 0
            local.get 1
            i32.store8)
          (func (export "fused_eqz_br_if") (param i32) (result i32)
            (local i32)
            block $taken
              local.get 0
              i32.load8_u
              local.tee 1
              i32.eqz
              br_if $taken
              local.get 1
              return
            end
            i32.const 0
            )
          (func (export "baseline_eqz_br_if") (param i32) (result i32)
            (local i32)
            block $taken
              local.get 0
              i32.load8_u
              local.tee 1
              i32.eqz
              i32.const 0
              i32.or
              br_if $taken
              local.get 1
              return
            end
            i32.const 0
            )
          (func (export "fused_compare_br_if") (param i32) (result i32)
            (local i32)
            block $taken
              local.get 0
              i32.load8_u
              local.tee 1
              i32.const 31
              i32.gt_u
              br_if $taken
              i32.const 0
              return
            end
            local.get 1)
          (func (export "baseline_compare_br_if") (param i32) (result i32)
            (local i32)
            block $taken
              local.get 0
              i32.load8_u
              local.tee 1
              i32.const 31
              i32.gt_u
              i32.const 0
              i32.or
              br_if $taken
              i32.const 0
              return
            end
            local.get 1))
        "#,
        &store,
        &registry,
    )
    .await;

    assert!(matches!(
        run_module_function(
            &instance,
            &store,
            "seed",
            &ResultValue::new(vec![WasmValue::I32(0), WasmValue::I32(0)]),
        )
        .await,
        VMResult::Success(_)
    ));
    assert_success(
        call_i32(
            &instance,
            &store,
            "fused_eqz_br_if",
            vec![WasmValue::I32(0)],
        )
        .await,
        0,
        "fused_eqz_br_if zero",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_eqz_br_if",
            vec![WasmValue::I32(0)],
        )
        .await,
        0,
        "baseline_eqz_br_if zero",
    );

    assert!(matches!(
        run_module_function(
            &instance,
            &store,
            "seed",
            &ResultValue::new(vec![WasmValue::I32(4), WasmValue::I32(55)]),
        )
        .await,
        VMResult::Success(_)
    ));
    assert_success(
        call_i32(
            &instance,
            &store,
            "fused_eqz_br_if",
            vec![WasmValue::I32(4)],
        )
        .await,
        55,
        "fused_eqz_br_if nonzero",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_eqz_br_if",
            vec![WasmValue::I32(4)],
        )
        .await,
        55,
        "baseline_eqz_br_if nonzero",
    );

    assert!(matches!(
        run_module_function(
            &instance,
            &store,
            "seed",
            &ResultValue::new(vec![WasmValue::I32(8), WasmValue::I32(48)]),
        )
        .await,
        VMResult::Success(_)
    ));
    assert_success(
        call_i32(
            &instance,
            &store,
            "fused_compare_br_if",
            vec![WasmValue::I32(8)],
        )
        .await,
        48,
        "fused_compare_br_if taken",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_compare_br_if",
            vec![WasmValue::I32(8)],
        )
        .await,
        48,
        "baseline_compare_br_if taken",
    );

    assert!(matches!(
        run_module_function(
            &instance,
            &store,
            "seed",
            &ResultValue::new(vec![WasmValue::I32(12), WasmValue::I32(7)]),
        )
        .await,
        VMResult::Success(_)
    ));
    assert_success(
        call_i32(
            &instance,
            &store,
            "fused_compare_br_if",
            vec![WasmValue::I32(12)],
        )
        .await,
        0,
        "fused_compare_br_if fallthrough",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_compare_br_if",
            vec![WasmValue::I32(12)],
        )
        .await,
        0,
        "baseline_compare_br_if fallthrough",
    );

    assert_memory_oob(
        call_i32(
            &instance,
            &store,
            "fused_compare_br_if",
            vec![WasmValue::I32(65536)],
        )
        .await,
        "fused_compare_br_if_oob",
    );
    assert_memory_oob(
        call_i32(
            &instance,
            &store,
            "baseline_compare_br_if",
            vec![WasmValue::I32(65536)],
        )
        .await,
        "baseline_compare_br_if_oob",
    );
}

#[tokio::test]
async fn producer_scalar_branch_and_float_select_superinstructions_match_unfused_semantics() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (func (export "seed8") (param i32 i32)
            local.get 0
            local.get 1
            i32.store8)
          (func (export "seedf32") (param i32 f32)
            local.get 0
            local.get 1
            f32.store)
          (func (export "seedf64") (param i32 f64)
            local.get 0
            local.get 1
            f64.store)
          (func (export "fused_mask_set") (param i32) (result i32)
            (local i32)
            local.get 0
            i32.load8_u
            i32.const 31
            i32.and
            local.set 1
            local.get 1)
          (func (export "baseline_mask_set") (param i32) (result i32)
            (local i32)
            local.get 0
            i32.load8_u
            i32.const 0
            i32.or
            i32.const 31
            i32.and
            local.set 1
            local.get 1)
          (func (export "fused_mask_tee") (param i32) (result i32)
            (local i32)
            local.get 0
            i32.load8_u
            i32.const 31
            i32.and
            local.tee 1)
          (func (export "baseline_mask_tee") (param i32) (result i32)
            (local i32)
            local.get 0
            i32.load8_u
            i32.const 0
            i32.or
            i32.const 31
            i32.and
            local.tee 1)
          (func (export "fused_mask_if") (param i32) (result i32)
            block
              local.get 0
              i32.load8_u
              i32.const 31
              i32.and
              i32.eqz
              if
                i32.const 7
                return
              end
            end
            i32.const 0)
          (func (export "baseline_mask_if") (param i32) (result i32)
            block
              local.get 0
              i32.load8_u
              i32.const 31
              i32.and
              i32.const 0
              i32.or
              i32.eqz
              if
                i32.const 7
                return
              end
            end
            i32.const 0)
          (func (export "fused_mask_br_if") (param i32) (result i32)
            block $exit
              local.get 0
              i32.load8_u
              i32.const 31
              i32.and
              br_if $exit
              i32.const 0
              return
            end
            i32.const 7)
          (func (export "baseline_mask_br_if") (param i32) (result i32)
            block $exit
              local.get 0
              i32.load8_u
              i32.const 31
              i32.and
              i32.const 0
              i32.or
              br_if $exit
              i32.const 0
              return
            end
            i32.const 7)
          (func (export "fused_f32_select") (param i32 i32 i32) (result i32)
            local.get 1
            local.get 2
            local.get 0
            f32.load
            f32.const 0
            f32.gt
            select)
          (func (export "baseline_f32_select") (param i32 i32 i32) (result i32) (local i32)
            local.get 0
            f32.load
            f32.const 0
            f32.gt
            local.set 3
            local.get 1
            local.get 2
            local.get 3
            select)
          (func (export "fused_f64_select") (param i32 i64 i64) (result i64)
            local.get 1
            local.get 2
            local.get 0
            f64.load
            f64.const 0
            f64.gt
            select)
          (func (export "baseline_f64_select") (param i32 i64 i64) (result i64) (local i32)
            local.get 0
            f64.load
            f64.const 0
            f64.gt
            local.set 3
            local.get 1
            local.get 2
            local.get 3
            select))
        "#,
        &store,
        &registry,
    )
    .await;

    assert!(matches!(
        run_module_function(
            &instance,
            &store,
            "seed8",
            &ResultValue::new(vec![WasmValue::I32(0), WasmValue::I32(63)]),
        )
        .await,
        VMResult::Success(_)
    ));

    assert_success(
        call_i32(&instance, &store, "fused_mask_set", vec![WasmValue::I32(0)]).await,
        31,
        "fused_mask_set",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_mask_set",
            vec![WasmValue::I32(0)],
        )
        .await,
        31,
        "baseline_mask_set",
    );
    assert_success(
        call_i32(&instance, &store, "fused_mask_tee", vec![WasmValue::I32(0)]).await,
        31,
        "fused_mask_tee",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_mask_tee",
            vec![WasmValue::I32(0)],
        )
        .await,
        31,
        "baseline_mask_tee",
    );
    assert_success(
        call_i32(&instance, &store, "fused_mask_if", vec![WasmValue::I32(0)]).await,
        0,
        "fused_mask_if nonzero",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_mask_if",
            vec![WasmValue::I32(0)],
        )
        .await,
        0,
        "baseline_mask_if nonzero",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "fused_mask_br_if",
            vec![WasmValue::I32(0)],
        )
        .await,
        7,
        "fused_mask_br_if nonzero",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_mask_br_if",
            vec![WasmValue::I32(0)],
        )
        .await,
        7,
        "baseline_mask_br_if nonzero",
    );

    assert!(matches!(
        run_module_function(
            &instance,
            &store,
            "seed8",
            &ResultValue::new(vec![WasmValue::I32(4), WasmValue::I32(0)]),
        )
        .await,
        VMResult::Success(_)
    ));

    assert_success(
        call_i32(&instance, &store, "fused_mask_if", vec![WasmValue::I32(4)]).await,
        7,
        "fused_mask_if zero",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_mask_if",
            vec![WasmValue::I32(4)],
        )
        .await,
        7,
        "baseline_mask_if zero",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "fused_mask_br_if",
            vec![WasmValue::I32(4)],
        )
        .await,
        0,
        "fused_mask_br_if zero",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_mask_br_if",
            vec![WasmValue::I32(4)],
        )
        .await,
        0,
        "baseline_mask_br_if zero",
    );

    assert!(matches!(
        run_module_function(
            &instance,
            &store,
            "seedf32",
            &ResultValue::new(vec![WasmValue::I32(16), WasmValue::F32(1.5)]),
        )
        .await,
        VMResult::Success(_)
    ));
    assert!(matches!(
        run_module_function(
            &instance,
            &store,
            "seedf64",
            &ResultValue::new(vec![WasmValue::I32(24), WasmValue::F64(-2.0)]),
        )
        .await,
        VMResult::Success(_)
    ));

    assert_success(
        call_i32(
            &instance,
            &store,
            "fused_f32_select",
            vec![WasmValue::I32(16), WasmValue::I32(11), WasmValue::I32(22)],
        )
        .await,
        11,
        "fused_f32_select",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_f32_select",
            vec![WasmValue::I32(16), WasmValue::I32(11), WasmValue::I32(22)],
        )
        .await,
        11,
        "baseline_f32_select",
    );
    assert_success(
        call_i64(
            &instance,
            &store,
            "fused_f64_select",
            vec![WasmValue::I32(24), WasmValue::I64(33), WasmValue::I64(44)],
        )
        .await,
        44,
        "fused_f64_select",
    );
    assert_success(
        call_i64(
            &instance,
            &store,
            "baseline_f64_select",
            vec![WasmValue::I32(24), WasmValue::I64(33), WasmValue::I64(44)],
        )
        .await,
        44,
        "baseline_f64_select",
    );

    assert_memory_oob(
        call_i32(
            &instance,
            &store,
            "fused_mask_set",
            vec![WasmValue::I32(65536)],
        )
        .await,
        "fused_mask_set_oob",
    );
    assert_memory_oob(
        call_i32(
            &instance,
            &store,
            "baseline_mask_set",
            vec![WasmValue::I32(65536)],
        )
        .await,
        "baseline_mask_set_oob",
    );
    assert_memory_oob(
        call_i32(
            &instance,
            &store,
            "fused_f32_select",
            vec![
                WasmValue::I32(65536),
                WasmValue::I32(11),
                WasmValue::I32(22),
            ],
        )
        .await,
        "fused_f32_select_oob",
    );
    assert_memory_oob(
        call_i32(
            &instance,
            &store,
            "baseline_f32_select",
            vec![
                WasmValue::I32(65536),
                WasmValue::I32(11),
                WasmValue::I32(22),
            ],
        )
        .await,
        "baseline_f32_select_oob",
    );
}

#[tokio::test]
async fn compare_superinstructions_match_unfused_semantics_and_float_edges() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "fused_i32_lt_s") (param i32 i32) (result i32) (local i32)
            local.get 0
            local.get 1
            i32.lt_s
            local.set 2
            local.get 2)
          (func (export "baseline_i32_lt_s") (param i32 i32) (result i32) (local i32)
            local.get 0
            local.get 1
            i32.lt_s
            i32.const 0
            i32.or
            local.set 2
            local.get 2)
          (func (export "fused_i32_lt_u") (param i32 i32) (result i32) (local i32)
            local.get 0
            local.get 1
            i32.lt_u
            local.set 2
            local.get 2)
          (func (export "baseline_i32_lt_u") (param i32 i32) (result i32) (local i32)
            local.get 0
            local.get 1
            i32.lt_u
            i32.const 0
            i32.or
            local.set 2
            local.get 2)
          (func (export "fused_i64_eq") (param i64 i64) (result i32) (local i32)
            local.get 0
            local.get 1
            i64.eq
            local.set 2
            local.get 2)
          (func (export "baseline_i64_eq") (param i64 i64) (result i32) (local i32)
            local.get 0
            local.get 1
            i64.eq
            i32.const 0
            i32.or
            local.set 2
            local.get 2)
          (func (export "fused_f32_lt_nan") (param f32) (result i32) (local i32)
            local.get 0
            f32.const nan
            f32.lt
            local.set 1
            local.get 1)
          (func (export "baseline_f32_lt_nan") (param f32) (result i32) (local i32)
            local.get 0
            f32.const nan
            f32.lt
            i32.const 0
            i32.or
            local.set 1
            local.get 1)
          (func (export "fused_f64_eq_zero") (param f64) (result i32) (local i32)
            local.get 0
            f64.const 0
            f64.eq
            local.set 1
            local.get 1)
          (func (export "baseline_f64_eq_zero") (param f64) (result i32) (local i32)
            local.get 0
            f64.const 0
            f64.eq
            i32.const 0
            i32.or
            local.set 1
            local.get 1)
          (func (export "fused_i64_ge_u_branch") (param i64) (result i32)
            block $taken
              local.get 0
              i64.const 7
              i64.ge_u
              br_if $taken
              i32.const 0
              return
            end
            i32.const 1)
          (func (export "baseline_i64_ge_u_branch") (param i64) (result i32)
            block $taken
              local.get 0
              i64.const 7
              i64.ge_u
              i32.const 0
              i32.or
              br_if $taken
              i32.const 0
              return
            end
            i32.const 1))
        "#,
        &store,
        &registry,
    )
    .await;

    let signed_args = vec![WasmValue::I32(-1), WasmValue::I32(1)];
    assert_success(
        call_i32(&instance, &store, "fused_i32_lt_s", signed_args.clone()).await,
        1,
        "fused_i32_lt_s",
    );
    assert_success(
        call_i32(&instance, &store, "baseline_i32_lt_s", signed_args.clone()).await,
        1,
        "baseline_i32_lt_s",
    );
    assert_success(
        call_i32(&instance, &store, "fused_i32_lt_u", signed_args.clone()).await,
        0,
        "fused_i32_lt_u",
    );
    assert_success(
        call_i32(&instance, &store, "baseline_i32_lt_u", signed_args).await,
        0,
        "baseline_i32_lt_u",
    );

    let i64_args = vec![WasmValue::I64(5), WasmValue::I64(5)];
    assert_success(
        call_i32(&instance, &store, "fused_i64_eq", i64_args.clone()).await,
        1,
        "fused_i64_eq",
    );
    assert_success(
        call_i32(&instance, &store, "baseline_i64_eq", i64_args).await,
        1,
        "baseline_i64_eq",
    );

    assert_success(
        call_i32(
            &instance,
            &store,
            "fused_f32_lt_nan",
            vec![WasmValue::F32(1.0)],
        )
        .await,
        0,
        "fused_f32_lt_nan",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_f32_lt_nan",
            vec![WasmValue::F32(1.0)],
        )
        .await,
        0,
        "baseline_f32_lt_nan",
    );

    assert_success(
        call_i32(
            &instance,
            &store,
            "fused_f64_eq_zero",
            vec![WasmValue::F64(-0.0)],
        )
        .await,
        1,
        "fused_f64_eq_zero",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_f64_eq_zero",
            vec![WasmValue::F64(-0.0)],
        )
        .await,
        1,
        "baseline_f64_eq_zero",
    );

    for value in [6_i64, 7_i64] {
        let expected = i32::from(value >= 7);
        assert_success(
            call_i32(
                &instance,
                &store,
                "fused_i64_ge_u_branch",
                vec![WasmValue::I64(value)],
            )
            .await,
            expected,
            "fused_i64_ge_u_branch",
        );
        assert_success(
            call_i32(
                &instance,
                &store,
                "baseline_i64_ge_u_branch",
                vec![WasmValue::I64(value)],
            )
            .await,
            expected,
            "baseline_i64_ge_u_branch",
        );
    }
}

#[tokio::test]
async fn integer_divrem_superinstructions_preserve_nontrap_and_trap_semantics() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "fused_i32_div_u") (param i32) (result i32)
            local.get 0
            i32.const 3
            i32.div_u
            local.set 0
            local.get 0)
          (func (export "baseline_i32_div_u") (param i32) (result i32)
            local.get 0
            i32.const 3
            i32.div_u
            i32.const 0
            i32.add
            local.set 0
            local.get 0)
          (func (export "fused_i32_div_zero") (param i32) (result i32)
            local.get 0
            i32.const 0
            i32.div_s
            local.set 0
            local.get 0)
          (func (export "baseline_i32_div_zero") (param i32) (result i32)
            local.get 0
            i32.const 0
            i32.div_s
            i32.const 0
            i32.add
            local.set 0
            local.get 0)
          (func (export "fused_i32_div_overflow") (param i32) (result i32)
            local.get 0
            i32.const -1
            i32.div_s
            local.set 0
            local.get 0)
          (func (export "baseline_i32_div_overflow") (param i32) (result i32)
            local.get 0
            i32.const -1
            i32.div_s
            i32.const 0
            i32.add
            local.set 0
            local.get 0)
          (func (export "fused_i32_rem_zero") (param i32) (result i32)
            local.get 0
            i32.const 0
            i32.rem_s
            local.set 0
            local.get 0)
          (func (export "baseline_i32_rem_zero") (param i32) (result i32)
            local.get 0
            i32.const 0
            i32.rem_s
            i32.const 0
            i32.add
            local.set 0
            local.get 0)
          (func (export "fused_i64_rem_u") (param i64) (result i64)
            local.get 0
            i64.const 5
            i64.rem_u
            local.set 0
            local.get 0)
          (func (export "baseline_i64_rem_u") (param i64) (result i64)
            local.get 0
            i64.const 5
            i64.rem_u
            i64.const 0
            i64.add
            local.set 0
            local.get 0)
          (func (export "fused_i64_div_zero") (param i64) (result i64)
            local.get 0
            i64.const 0
            i64.div_s
            local.set 0
            local.get 0)
          (func (export "baseline_i64_div_zero") (param i64) (result i64)
            local.get 0
            i64.const 0
            i64.div_s
            i64.const 0
            i64.add
            local.set 0
            local.get 0)
          (func (export "fused_i64_div_overflow") (param i64) (result i64)
            local.get 0
            i64.const -1
            i64.div_s
            local.set 0
            local.get 0)
          (func (export "baseline_i64_div_overflow") (param i64) (result i64)
            local.get 0
            i64.const -1
            i64.div_s
            i64.const 0
            i64.add
            local.set 0
            local.get 0)
          (func (export "fused_i64_rem_zero") (param i64) (result i64)
            local.get 0
            i64.const 0
            i64.rem_s
            local.set 0
            local.get 0)
          (func (export "baseline_i64_rem_zero") (param i64) (result i64)
            local.get 0
            i64.const 0
            i64.rem_s
            i64.const 0
            i64.add
            local.set 0
            local.get 0))
        "#,
        &store,
        &registry,
    )
    .await;

    assert_success(
        call_i32(
            &instance,
            &store,
            "fused_i32_div_u",
            vec![WasmValue::I32(14)],
        )
        .await,
        4,
        "fused_i32_div_u",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_i32_div_u",
            vec![WasmValue::I32(14)],
        )
        .await,
        4,
        "baseline_i32_div_u",
    );
    assert_success(
        call_i64(
            &instance,
            &store,
            "fused_i64_rem_u",
            vec![WasmValue::I64(42)],
        )
        .await,
        2,
        "fused_i64_rem_u",
    );
    assert_success(
        call_i64(
            &instance,
            &store,
            "baseline_i64_rem_u",
            vec![WasmValue::I64(42)],
        )
        .await,
        2,
        "baseline_i64_rem_u",
    );

    for name in [
        "fused_i32_div_zero",
        "baseline_i32_div_zero",
        "fused_i32_rem_zero",
        "baseline_i32_rem_zero",
    ] {
        assert_invalid_operand(
            call_i32(&instance, &store, name, vec![WasmValue::I32(10)]).await,
            name,
        );
    }

    for name in ["fused_i32_div_overflow", "baseline_i32_div_overflow"] {
        assert_invalid_operand(
            call_i32(&instance, &store, name, vec![WasmValue::I32(i32::MIN)]).await,
            name,
        );
    }

    for name in [
        "fused_i64_div_zero",
        "baseline_i64_div_zero",
        "fused_i64_rem_zero",
        "baseline_i64_rem_zero",
    ] {
        assert_invalid_operand(
            call_i64(&instance, &store, name, vec![WasmValue::I64(10)]).await,
            name,
        );
    }

    for name in ["fused_i64_div_overflow", "baseline_i64_div_overflow"] {
        assert_invalid_operand(
            call_i64(&instance, &store, name, vec![WasmValue::I64(i64::MIN)]).await,
            name,
        );
    }
}

#[tokio::test]
async fn typed_memory_superinstructions_match_unfused_semantics() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data (i32.const 0) "\ff\ff\ff\ff\34\12\00\00\00\00\00\00\00\00\f8\3f")
          (func (export "fused_i64_load16_s") (param i32) (result i64)
            local.get 0
            i64.load16_s)
          (func (export "baseline_i64_load16_s") (param i32) (result i64)
            local.get 0
            i32.const 0
            i32.add
            i64.load16_s)
          (func (export "fused_i64_load32_u") (param i32) (result i64)
            local.get 0
            i64.load32_u)
          (func (export "baseline_i64_load32_u") (param i32) (result i64)
            local.get 0
            i32.const 0
            i32.add
            i64.load32_u)
          (func (export "fused_f64_load") (param i32) (result f64)
            local.get 0
            f64.load)
          (func (export "baseline_f64_load") (param i32) (result f64)
            local.get 0
            i32.const 0
            i32.add
            f64.load)
          (func (export "fused_i64_store32") (param i32 i64)
            local.get 0
            local.get 1
            i64.store32)
          (func (export "baseline_i64_store32") (param i32 i64)
            local.get 0
            i32.const 0
            i32.add
            local.get 1
            i64.store32)
          (func (export "fused_f64_store") (param i32 f64)
            local.get 0
            local.get 1
            f64.store)
          (func (export "baseline_f64_store") (param i32 f64)
            local.get 0
            i32.const 0
            i32.add
            local.get 1
            f64.store)
          (func (export "load64") (param i32) (result i64)
            local.get 0
            i64.load)
          (func (export "loadf64") (param i32) (result f64)
            local.get 0
            f64.load)
          (func (export "const_store64") (param i64)
            i32.const 24
            local.get 0
            i64.store)
          (func (export "const_load64") (result i64)
            i32.const 24
            i64.load)
          (func (export "const_storef64") (param f64)
            i32.const 32
            local.get 0
            f64.store)
          (func (export "const_loadf64") (result f64)
            i32.const 32
            f64.load))
        "#,
        &store,
        &registry,
    )
    .await;

    assert_success(
        call_i64(
            &instance,
            &store,
            "fused_i64_load16_s",
            vec![WasmValue::I32(0)],
        )
        .await,
        -1,
        "fused_i64_load16_s",
    );
    assert_success(
        call_i64(
            &instance,
            &store,
            "baseline_i64_load16_s",
            vec![WasmValue::I32(0)],
        )
        .await,
        -1,
        "baseline_i64_load16_s",
    );
    assert_success(
        call_i64(
            &instance,
            &store,
            "fused_i64_load32_u",
            vec![WasmValue::I32(0)],
        )
        .await,
        0xffff_ffff_u64 as i64,
        "fused_i64_load32_u",
    );
    assert_success(
        call_i64(
            &instance,
            &store,
            "baseline_i64_load32_u",
            vec![WasmValue::I32(0)],
        )
        .await,
        0xffff_ffff_u64 as i64,
        "baseline_i64_load32_u",
    );
    assert_success(
        call_f64_bits(&instance, &store, "fused_f64_load", vec![WasmValue::I32(8)]).await,
        1.5f64.to_bits(),
        "fused_f64_load",
    );
    assert_success(
        call_f64_bits(
            &instance,
            &store,
            "baseline_f64_load",
            vec![WasmValue::I32(8)],
        )
        .await,
        1.5f64.to_bits(),
        "baseline_f64_load",
    );

    for (name, addr) in [("fused_i64_store32", 40), ("baseline_i64_store32", 48)] {
        assert!(
            matches!(
                run_module_function(
                    &instance,
                    &store,
                    name,
                    &ResultValue::new(vec![
                        WasmValue::I32(addr),
                        WasmValue::I64(0x1122_3344_5566_7788),
                    ]),
                )
                .await,
                VMResult::Success(_)
            ),
            "{name} must succeed"
        );
    }
    let truncated = 0x0000_0000_5566_7788_u64 as i64;
    assert_success(
        call_i64(&instance, &store, "load64", vec![WasmValue::I32(40)]).await,
        truncated,
        "load64@40",
    );
    assert_success(
        call_i64(&instance, &store, "load64", vec![WasmValue::I32(48)]).await,
        truncated,
        "load64@48",
    );

    for (name, addr) in [("fused_f64_store", 56), ("baseline_f64_store", 64)] {
        assert!(
            matches!(
                run_module_function(
                    &instance,
                    &store,
                    name,
                    &ResultValue::new(vec![WasmValue::I32(addr), WasmValue::F64(2.25)]),
                )
                .await,
                VMResult::Success(_)
            ),
            "{name} must succeed"
        );
    }
    assert_success(
        call_f64_bits(&instance, &store, "loadf64", vec![WasmValue::I32(56)]).await,
        2.25f64.to_bits(),
        "loadf64@56",
    );
    assert_success(
        call_f64_bits(&instance, &store, "loadf64", vec![WasmValue::I32(64)]).await,
        2.25f64.to_bits(),
        "loadf64@64",
    );

    assert!(matches!(
        run_module_function(
            &instance,
            &store,
            "const_store64",
            &ResultValue::new(vec![WasmValue::I64(0x0102_0304_0506_0708)]),
        )
        .await,
        VMResult::Success(_)
    ));
    assert_success(
        call_i64(&instance, &store, "const_load64", vec![]).await,
        0x0102_0304_0506_0708,
        "const_load64",
    );
    assert!(matches!(
        run_module_function(
            &instance,
            &store,
            "const_storef64",
            &ResultValue::new(vec![WasmValue::F64(6.5)]),
        )
        .await,
        VMResult::Success(_)
    ));
    assert_success(
        call_f64_bits(&instance, &store, "const_loadf64", vec![]).await,
        6.5f64.to_bits(),
        "const_loadf64",
    );
}

#[tokio::test]
async fn typed_memory_superinstructions_preserve_traps() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (func (export "fused_i64_load_oob") (param i32) (result i64)
            local.get 0
            i64.load)
          (func (export "baseline_i64_load_oob") (param i32) (result i64)
            local.get 0
            i32.const 0
            i32.add
            i64.load)
          (func (export "fused_i64_load_overflow") (param i32) (result i64)
            local.get 0
            i64.load offset=1)
          (func (export "baseline_i64_load_overflow") (param i32) (result i64)
            local.get 0
            i32.const 0
            i32.add
            i64.load offset=1)
          (func (export "fused_f64_store_oob") (param i32 f64)
            local.get 0
            local.get 1
            f64.store)
          (func (export "baseline_f64_store_oob") (param i32 f64)
            local.get 0
            i32.const 0
            i32.add
            local.get 1
            f64.store)
          (func (export "fused_f64_store_overflow") (param i32 f64)
            local.get 0
            local.get 1
            f64.store offset=1)
          (func (export "baseline_f64_store_overflow") (param i32 f64)
            local.get 0
            i32.const 0
            i32.add
            local.get 1
            f64.store offset=1)
          (func (export "const_i64_load_oob") (result i64)
            i32.const 65536
            i64.load)
          (func (export "const_f64_store_overflow") (param f64)
            i32.const -1
            local.get 0
            f64.store offset=1))
        "#,
        &store,
        &registry,
    )
    .await;

    for name in ["fused_i64_load_oob", "baseline_i64_load_oob"] {
        assert_memory_oob(
            call_i64(&instance, &store, name, vec![WasmValue::I32(65536)]).await,
            name,
        );
    }

    for name in ["fused_i64_load_overflow", "baseline_i64_load_overflow"] {
        assert_memory_oob(
            call_i64(&instance, &store, name, vec![WasmValue::I32(-1)]).await,
            name,
        );
    }

    for (name, addr) in [
        ("fused_f64_store_oob", 65536),
        ("baseline_f64_store_oob", 65536),
        ("fused_f64_store_overflow", -1),
        ("baseline_f64_store_overflow", -1),
    ] {
        assert_memory_oob(
            run_module_function(
                &instance,
                &store,
                name,
                &ResultValue::new(vec![WasmValue::I32(addr), WasmValue::F64(1.0)]),
            )
            .await,
            name,
        );
    }

    assert_memory_oob(
        call_i64(&instance, &store, "const_i64_load_oob", vec![]).await,
        "const_i64_load_oob",
    );
    assert_memory_oob(
        run_module_function(
            &instance,
            &store,
            "const_f64_store_overflow",
            &ResultValue::new(vec![WasmValue::F64(1.0)]),
        )
        .await,
        "const_f64_store_overflow",
    );
}
