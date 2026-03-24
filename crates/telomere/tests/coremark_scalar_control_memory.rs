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

fn assert_success(result: VMResult<i32>, expected: i32, name: &str) {
    match result {
        VMResult::Success(actual) => assert_eq!(actual, expected, "{name} returned wrong value"),
        other => panic!("expected Success({expected:?}) from {name}, got {other:?}"),
    }
}

#[tokio::test]
async fn coremark_scalar_control_superinstructions_preserve_semantics() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "fused_const_and_eqz_if") (param i32) (result i32)
            local.get 0
            i32.const 31
            i32.and
            i32.eqz
            if (result i32)
              i32.const 11
            else
              i32.const 22
            end)
          (func (export "baseline_const_and_eqz_if") (param i32) (result i32)
            local.get 0
            i32.const 0
            i32.add
            i32.const 31
            i32.and
            i32.eqz
            if (result i32)
              i32.const 11
            else
              i32.const 22
            end)
          (func (export "fused_const_and_br_if") (param i32) (result i32)
            block $exit
              local.get 0
              i32.const 31
              i32.and
              br_if $exit
              i32.const 7
              return
            end
            i32.const 9)
          (func (export "baseline_const_and_br_if") (param i32) (result i32)
            block $exit
              local.get 0
              i32.const 0
              i32.add
              i32.const 31
              i32.and
              br_if $exit
              i32.const 7
              return
            end
            i32.const 9)
          (func (export "fused_const_scalar_set") (param i32) (result i32)
            (local i32)
            local.get 0
            i32.const 5
            i32.add
            local.set 1
            local.get 1)
          (func (export "baseline_const_scalar_set") (param i32) (result i32)
            (local i32)
            local.get 0
            i32.const 0
            i32.add
            i32.const 5
            i32.add
            local.set 1
            local.get 1)
          (func (export "fused_const_scalar_tee") (param i32) (result i32)
            (local i32)
            local.get 0
            i32.const 5
            i32.add
            local.tee 1)
          (func (export "baseline_const_scalar_tee") (param i32) (result i32)
            (local i32)
            local.get 0
            i32.const 0
            i32.add
            i32.const 5
            i32.add
            local.tee 1))
        "#,
        &store,
        &registry,
    )
    .await;

    for value in [0, 4, 31, 32] {
        let expected = if value & 31 == 0 { 11 } else { 22 };
        assert_success(
            call_i32(
                &instance,
                &store,
                "fused_const_and_eqz_if",
                vec![WasmValue::I32(value)],
            )
            .await,
            expected,
            "fused_const_and_eqz_if",
        );
        assert_success(
            call_i32(
                &instance,
                &store,
                "baseline_const_and_eqz_if",
                vec![WasmValue::I32(value)],
            )
            .await,
            expected,
            "baseline_const_and_eqz_if",
        );
    }

    for value in [0, 1, 32, 63] {
        let expected = if value & 31 == 0 { 7 } else { 9 };
        assert_success(
            call_i32(
                &instance,
                &store,
                "fused_const_and_br_if",
                vec![WasmValue::I32(value)],
            )
            .await,
            expected,
            "fused_const_and_br_if",
        );
        assert_success(
            call_i32(
                &instance,
                &store,
                "baseline_const_and_br_if",
                vec![WasmValue::I32(value)],
            )
            .await,
            expected,
            "baseline_const_and_br_if",
        );
    }

    for value in [0, 17, 1024] {
        let expected = value + 5;
        assert_success(
            call_i32(
                &instance,
                &store,
                "fused_const_scalar_set",
                vec![WasmValue::I32(value)],
            )
            .await,
            expected,
            "fused_const_scalar_set",
        );
        assert_success(
            call_i32(
                &instance,
                &store,
                "baseline_const_scalar_set",
                vec![WasmValue::I32(value)],
            )
            .await,
            expected,
            "baseline_const_scalar_set",
        );
        assert_success(
            call_i32(
                &instance,
                &store,
                "fused_const_scalar_tee",
                vec![WasmValue::I32(value)],
            )
            .await,
            expected,
            "fused_const_scalar_tee",
        );
        assert_success(
            call_i32(
                &instance,
                &store,
                "baseline_const_scalar_tee",
                vec![WasmValue::I32(value)],
            )
            .await,
            expected,
            "baseline_const_scalar_tee",
        );
    }
}

#[tokio::test]
async fn coremark_memory_compare_branch_select_superinstructions_preserve_semantics() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (data (i32.const 0) "\ff\ff\00\00\34\12\00\00\ff\00\00\00")
          (func (export "fused_load8_u_br_if") (param i32) (result i32)
            block $exit
              local.get 0
              i32.load8_u
              i32.const 255
              i32.eq
              br_if $exit
              i32.const 7
              return
            end
            i32.const 9)
          (func (export "baseline_load8_u_br_if") (param i32) (result i32)
            block $exit
              local.get 0
              i32.const 0
              i32.add
              i32.load8_u
              i32.const 255
              i32.eq
              br_if $exit
              i32.const 7
              return
            end
            i32.const 9)
          (func (export "fused_load16_u_select") (param i32 i32 i32) (result i32)
            local.get 1
            local.get 2
            local.get 0
            i32.load16_u
            i32.const 4660
            i32.eq
            select)
          (func (export "baseline_load16_u_select") (param i32 i32 i32) (result i32)
            local.get 1
            local.get 2
            local.get 0
            i32.const 0
            i32.add
            i32.load16_u
            i32.const 4660
            i32.eq
            select)
          (func (export "fused_load16_s_br_if") (param i32) (result i32)
            block $exit
              local.get 0
              i32.load16_s
              i32.const -1
              i32.eq
              br_if $exit
              i32.const 7
              return
            end
            i32.const 9)
          (func (export "baseline_load16_s_br_if") (param i32) (result i32)
            block $exit
              local.get 0
              i32.const 0
              i32.add
              i32.load16_s
              i32.const -1
              i32.eq
              br_if $exit
              i32.const 7
              return
            end
            i32.const 9)
          (func (export "fused_select_local_tee") (param i32 i32 i32) (result i32)
            (local i32)
            local.get 1
            local.get 2
            local.get 0
            select
            local.tee 3)
          (func (export "baseline_select_local_tee") (param i32 i32 i32) (result i32)
            (local i32)
            local.get 1
            local.get 2
            local.get 0
            select
            local.set 3
            local.get 3))
        "#,
        &store,
        &registry,
    )
    .await;

    assert_success(
        call_i32(
            &instance,
            &store,
            "fused_load8_u_br_if",
            vec![WasmValue::I32(0)],
        )
        .await,
        9,
        "fused_load8_u_br_if@0",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_load8_u_br_if",
            vec![WasmValue::I32(0)],
        )
        .await,
        9,
        "baseline_load8_u_br_if@0",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "fused_load8_u_br_if",
            vec![WasmValue::I32(9)],
        )
        .await,
        7,
        "fused_load8_u_br_if@9",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_load8_u_br_if",
            vec![WasmValue::I32(9)],
        )
        .await,
        7,
        "baseline_load8_u_br_if@9",
    );

    assert_success(
        call_i32(
            &instance,
            &store,
            "fused_load16_u_select",
            vec![WasmValue::I32(4), WasmValue::I32(11), WasmValue::I32(22)],
        )
        .await,
        11,
        "fused_load16_u_select",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_load16_u_select",
            vec![WasmValue::I32(4), WasmValue::I32(11), WasmValue::I32(22)],
        )
        .await,
        11,
        "baseline_load16_u_select",
    );

    assert_success(
        call_i32(
            &instance,
            &store,
            "fused_load16_s_br_if",
            vec![WasmValue::I32(0)],
        )
        .await,
        9,
        "fused_load16_s_br_if@0",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_load16_s_br_if",
            vec![WasmValue::I32(0)],
        )
        .await,
        9,
        "baseline_load16_s_br_if@0",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "fused_load16_s_br_if",
            vec![WasmValue::I32(12)],
        )
        .await,
        7,
        "fused_load16_s_br_if@12",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_load16_s_br_if",
            vec![WasmValue::I32(12)],
        )
        .await,
        7,
        "baseline_load16_s_br_if@12",
    );

    assert_success(
        call_i32(
            &instance,
            &store,
            "fused_select_local_tee",
            vec![WasmValue::I32(1), WasmValue::I32(11), WasmValue::I32(22)],
        )
        .await,
        11,
        "fused_select_local_tee",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "baseline_select_local_tee",
            vec![WasmValue::I32(1), WasmValue::I32(11), WasmValue::I32(22)],
        )
        .await,
        11,
        "baseline_select_local_tee",
    );
}
