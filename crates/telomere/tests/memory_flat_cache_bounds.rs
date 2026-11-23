//! Boundary tests for the frame-cached flat (base, committed length) view
//! used by scalar memory handlers (#187).
//!
//! These tests pin the exact trap boundary of the flattened single-compare
//! check: an access of width `N` at `committed_len - N` succeeds and at
//! `committed_len - N + 1` traps with `MemoryIndexOutOfRange`, and a
//! `memory.grow` inside a frame refreshes the cached length so later
//! accesses in the *same* frame observe the grown memory — neither stale
//! (which would trap valid accesses) nor over-approximated (which would
//! let out-of-bounds accesses through).

mod common;

use common::instantiate_wat;
use telomere::{run_module_function, Registry, ResultValue, Store, VMResult, WasmValue};

const PAGE: i32 = 65536;

async fn call(
    store: &Store,
    instance: &telomere::InstanceHandle,
    name: &str,
    args: Vec<WasmValue>,
) -> VMResult<ResultValue> {
    run_module_function(instance, store, name, &ResultValue::new(args)).await
}

fn assert_oob(result: VMResult<ResultValue>) {
    assert!(
        matches!(result, VMResult::MemoryIndexOutOfRange),
        "expected MemoryIndexOutOfRange, got {result:?}"
    );
}

fn assert_i32(result: VMResult<ResultValue>, expected: i32) {
    match result {
        VMResult::Success(values) => match values.iter().next() {
            Some(WasmValue::I32(value)) => assert_eq!(*value, expected),
            other => panic!("expected i32 result {expected}, got {other:?}"),
        },
        other => panic!("expected Success({expected}), got {other:?}"),
    }
}

#[tokio::test]
async fn scalar_accesses_stop_exactly_at_committed_len() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (func (export "load32") (param i32) (result i32)
            local.get 0
            i32.load)
          (func (export "store32") (param i32 i32)
            local.get 0
            local.get 1
            i32.store)
          (func (export "load64") (param i32) (result i64)
            local.get 0
            i64.load)
          (func (export "store64") (param i32 i64)
            local.get 0
            local.get 1
            i64.store)
          (func (export "load8") (param i32) (result i32)
            local.get 0
            i32.load8_u)
          (func (export "load32_offset2") (param i32) (result i32)
            local.get 0
            i32.load offset=2))
        "#,
        &store,
        &registry,
    )
    .await;

    // Width 4: committed_len - 4 succeeds, committed_len - 4 + 1 traps.
    assert!(matches!(
        call(
            &store,
            &instance,
            "store32",
            vec![WasmValue::I32(PAGE - 4), WasmValue::I32(0x1122_3344)],
        )
        .await,
        VMResult::Success(_)
    ));
    assert_i32(
        call(&store, &instance, "load32", vec![WasmValue::I32(PAGE - 4)]).await,
        0x1122_3344,
    );
    assert_oob(
        call(
            &store,
            &instance,
            "store32",
            vec![WasmValue::I32(PAGE - 4 + 1), WasmValue::I32(1)],
        )
        .await,
    );
    assert_oob(
        call(
            &store,
            &instance,
            "load32",
            vec![WasmValue::I32(PAGE - 4 + 1)],
        )
        .await,
    );

    // Width 8: committed_len - 8 succeeds, committed_len - 8 + 1 traps.
    assert!(matches!(
        call(
            &store,
            &instance,
            "store64",
            vec![
                WasmValue::I32(PAGE - 8),
                WasmValue::I64(0x1122_3344_5566_7788)
            ],
        )
        .await,
        VMResult::Success(_)
    ));
    match call(&store, &instance, "load64", vec![WasmValue::I32(PAGE - 8)]).await {
        VMResult::Success(values) => match values.iter().next() {
            Some(WasmValue::I64(value)) => assert_eq!(*value, 0x1122_3344_5566_7788),
            other => panic!("expected i64 result, got {other:?}"),
        },
        other => panic!("expected Success, got {other:?}"),
    }
    assert_oob(
        call(
            &store,
            &instance,
            "load64",
            vec![WasmValue::I32(PAGE - 8 + 1)],
        )
        .await,
    );

    // Width 1: the last committed byte is readable (it holds the top byte
    // of the 0x1122_3344 stored at PAGE - 4), the first byte past it traps.
    assert_i32(
        call(&store, &instance, "load8", vec![WasmValue::I32(PAGE - 1)]).await,
        0x11,
    );
    assert_oob(call(&store, &instance, "load8", vec![WasmValue::I32(PAGE)]).await);

    // The memarg offset participates in the same single comparison:
    // addr + 2 + 4 == committed_len succeeds, addr + 2 + 4 == len + 1 traps.
    assert_i32(
        call(
            &store,
            &instance,
            "load32_offset2",
            vec![WasmValue::I32(PAGE - 6)],
        )
        .await,
        0x1122_3344,
    );
    assert_oob(
        call(
            &store,
            &instance,
            "load32_offset2",
            vec![WasmValue::I32(PAGE - 5)],
        )
        .await,
    );
}

#[tokio::test]
async fn grow_in_same_frame_refreshes_the_cached_length() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1 3)
          (func (export "load32") (param i32) (result i32)
            local.get 0
            i32.load)
          (func (export "grow_then_load") (result i32)
            ;; Grow from 1 page to 2 pages, then — still in this frame —
            ;; load at the new committed_len - 4 boundary.
            i32.const 1
            memory.grow
            drop
            i32.const 131068
            i32.load)
          (func (export "grow_then_store_load") (result i32)
            i32.const 1
            memory.grow
            drop
            i32.const 131068
            i32.const 4242
            i32.store
            i32.const 131068
            i32.load))
        "#,
        &store,
        &registry,
    )
    .await;

    // Before any grow, the second page is out of bounds.
    assert_oob(call(&store, &instance, "load32", vec![WasmValue::I32(131068)]).await);

    // In the same frame as the grow, the new boundary is reachable...
    assert_i32(call(&store, &instance, "grow_then_load", vec![]).await, 0);
    // ...for stores as well as loads: this export grows once more (the
    // instance is now at its 3-page cap) and round-trips a value through
    // the page-2 boundary address in the same frame.
    assert_i32(
        call(&store, &instance, "grow_then_store_load", vec![]).await,
        4242,
    );

    // The refreshed length is exact, not over-approximated: on a fresh
    // instance, one byte past the grown boundary still traps.
    let store2 = Store::new();
    let registry2 = Registry::new();
    let instance2 = instantiate_wat(
        r#"
        (module
          (memory 1 3)
          (func (export "grow_then_load_past_new_len") (result i32)
            i32.const 1
            memory.grow
            drop
            i32.const 131069
            i32.load))
        "#,
        &store2,
        &registry2,
    )
    .await;
    assert_oob(call(&store2, &instance2, "grow_then_load_past_new_len", vec![]).await);

    // A failed grow must leave the cached length at the old value. On a
    // fresh 1-page instance, growing by 3 pages exceeds the 3-page cap and
    // returns -1; the old boundary must then behave exactly as before.
    let store3 = Store::new();
    let registry3 = Registry::new();
    let instance3 = instantiate_wat(
        r#"
        (module
          (memory 1 3)
          (func (export "failed_grow_then_load") (param i32) (result i32)
            i32.const 3
            memory.grow
            i32.const -1
            i32.ne
            if
              unreachable
            end
            local.get 0
            i32.load))
        "#,
        &store3,
        &registry3,
    )
    .await;
    assert_i32(
        call(
            &store3,
            &instance3,
            "failed_grow_then_load",
            vec![WasmValue::I32(PAGE - 4)],
        )
        .await,
        0,
    );
    assert_oob(
        call(
            &store3,
            &instance3,
            "failed_grow_then_load",
            vec![WasmValue::I32(PAGE - 4 + 1)],
        )
        .await,
    );
}
