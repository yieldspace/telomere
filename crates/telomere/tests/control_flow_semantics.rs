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

fn assert_success(result: VMResult<i32>, expected: i32, name: &str) {
    match result {
        VMResult::Success(actual) => assert_eq!(actual, expected, "{name} returned wrong value"),
        other => panic!("expected Success({expected}) from {name}, got {other:?}"),
    }
}

#[tokio::test]
async fn derived_control_flow_rewrites_preserve_if_br_table_and_specialized_branch_semantics() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (func (export "if_else") (param i32) (result i32)
            block (result i32)
              local.get 0
              if (result i32)
                i32.const 11
              else
                i32.const 22
              end
            end)
          (func (export "table") (param i32) (result i32)
            (local i32)
            i32.const 300
            local.set 1
            block
              block
                block
                  local.get 0
                  br_table 0 1 2
                end
                i32.const 100
                local.set 1
                br 1
              end
              i32.const 200
              local.set 1
              br 0
            end
            local.get 1)
          (func (export "specialized_branch") (param i32) (result i32)
            block
              local.get 0
              i32.eqz
              br_if 0
              i32.const 9
              return
            end
            i32.const 4))
        "#,
        &store,
        &registry,
    )
    .await;

    assert_success(
        call_i32(&instance, &store, "if_else", vec![WasmValue::I32(0)]).await,
        22,
        "if_else false",
    );
    assert_success(
        call_i32(&instance, &store, "if_else", vec![WasmValue::I32(1)]).await,
        11,
        "if_else true",
    );

    assert_success(
        call_i32(&instance, &store, "table", vec![WasmValue::I32(0)]).await,
        100,
        "table arm0",
    );
    assert_success(
        call_i32(&instance, &store, "table", vec![WasmValue::I32(1)]).await,
        200,
        "table arm1",
    );
    assert_success(
        call_i32(&instance, &store, "table", vec![WasmValue::I32(9)]).await,
        300,
        "table default",
    );

    assert_success(
        call_i32(
            &instance,
            &store,
            "specialized_branch",
            vec![WasmValue::I32(0)],
        )
        .await,
        4,
        "specialized_branch taken",
    );
    assert_success(
        call_i32(
            &instance,
            &store,
            "specialized_branch",
            vec![WasmValue::I32(7)],
        )
        .await,
        9,
        "specialized_branch fallthrough",
    );
}
