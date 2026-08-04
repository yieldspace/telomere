#![cfg(feature = "threads")]

mod common;

use common::instantiate_wat;
use std::sync::Arc;
use telomere::{
    host_abi::SharedMemoryObject,
    run_module_function,
    unstable_internals::{
        new_shared_memory, shared_atomic_rmw_u32, shared_register_wait32, AtomicRmwOperation,
        SharedWaitState,
    },
    InstanceHandle, Registry, ResultValue, Store, VMResult, WasmValue,
};

fn unwrap_success<T: std::fmt::Debug>(result: VMResult<T>) -> T {
    match result {
        VMResult::Success(value) => value,
        other => panic!("expected success, got {other:?}"),
    }
}

fn shared_memory() -> Arc<SharedMemoryObject> {
    unwrap_success(new_shared_memory(1, 1))
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
        VMResult::TableIndexOutOfRange => VMResult::TableIndexOutOfRange,
        VMResult::CallIndirectInvalidType => VMResult::CallIndirectInvalidType,
        VMResult::TableUninitialized => VMResult::TableUninitialized,
        VMResult::Unlinkable => VMResult::Unlinkable,
        VMResult::MemoryAllocationFailed => VMResult::MemoryAllocationFailed,
        VMResult::InvalidOperand => VMResult::InvalidOperand,
        VMResult::UnalignedAtomic => VMResult::UnalignedAtomic,
        VMResult::Unimplemented => VMResult::Unimplemented,
        VMResult::FuelExhausted => VMResult::FuelExhausted,
        VMResult::Cancelled => VMResult::Cancelled,
    }
}

async fn call_i64(
    instance: &InstanceHandle,
    store: &Store,
    name: &str,
    args: Vec<WasmValue>,
) -> VMResult<i64> {
    match run_module_function(instance, store, name, &ResultValue::new(args)).await {
        VMResult::Success(values) => match values.iter().next() {
            Some(WasmValue::I64(value)) => VMResult::Success(*value),
            other => panic!("expected i64 result from {name}, got {other:?}"),
        },
        VMResult::Unreachable => VMResult::Unreachable,
        VMResult::StackOverflow => VMResult::StackOverflow,
        VMResult::MemoryIndexOutOfRange => VMResult::MemoryIndexOutOfRange,
        VMResult::TableIndexOutOfRange => VMResult::TableIndexOutOfRange,
        VMResult::CallIndirectInvalidType => VMResult::CallIndirectInvalidType,
        VMResult::TableUninitialized => VMResult::TableUninitialized,
        VMResult::Unlinkable => VMResult::Unlinkable,
        VMResult::MemoryAllocationFailed => VMResult::MemoryAllocationFailed,
        VMResult::InvalidOperand => VMResult::InvalidOperand,
        VMResult::UnalignedAtomic => VMResult::UnalignedAtomic,
        VMResult::Unimplemented => VMResult::Unimplemented,
        VMResult::FuelExhausted => VMResult::FuelExhausted,
        VMResult::Cancelled => VMResult::Cancelled,
    }
}

#[tokio::test]
async fn unshared_wait_traps_and_notify_returns_zero() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (func (export "notify") (param i32 i32) (result i32)
            local.get 0
            local.get 1
            memory.atomic.notify)
          (func (export "wait32") (param i32 i32 i64) (result i32)
            local.get 0
            local.get 1
            local.get 2
            memory.atomic.wait32))
        "#,
        &store,
        &registry,
    )
    .await;

    assert_eq!(
        unwrap_success(
            call_i32(
                &instance,
                &store,
                "notify",
                vec![WasmValue::I32(0), WasmValue::I32(3)],
            )
            .await,
        ),
        0
    );
    assert!(matches!(
        call_i32(
            &instance,
            &store,
            "wait32",
            vec![WasmValue::I32(0), WasmValue::I32(0), WasmValue::I64(0),],
        )
        .await,
        VMResult::InvalidOperand
    ));
}

#[tokio::test]
async fn shared_wait_notify_is_fifo_and_timeout_removes_waiter() {
    let shared = shared_memory();
    unwrap_success(shared.atomic_store_u32(0, 7));

    assert!(matches!(
        unwrap_success(shared_register_wait32(&shared, 0, 99)),
        SharedWaitState::NotEqual
    ));

    let first = match unwrap_success(shared_register_wait32(&shared, 0, 7)) {
        SharedWaitState::Pending(wait) => wait,
        SharedWaitState::NotEqual => panic!("expected pending wait"),
    };
    let second = match unwrap_success(shared_register_wait32(&shared, 0, 7)) {
        SharedWaitState::Pending(wait) => wait,
        SharedWaitState::NotEqual => panic!("expected pending wait"),
    };

    assert_eq!(unwrap_success(shared.notify_waiters(0, 1)), 1);
    assert_eq!(first.wait_result(shared.clone(), -1).await, 0);
    assert_eq!(second.wait_result(shared.clone(), 0).await, 2);
    assert_eq!(unwrap_success(shared.notify_waiters(0, 1)), 0);
}

#[test]
fn shared_atomic_rmw_cmpxchg_and_alignment_follow_contracts() {
    let shared = shared_memory();
    unwrap_success(shared.atomic_store_u32(0, 0x1111_1111));

    assert_eq!(
        unwrap_success(shared_atomic_rmw_u32(
            &shared,
            0,
            AtomicRmwOperation::Add,
            1,
        )),
        0x1111_1111
    );
    assert_eq!(unwrap_success(shared.atomic_load_u32(0)), 0x1111_1112);

    assert_eq!(
        unwrap_success(shared.atomic_cmpxchg_u32(0, 0x2222_2222, 0x3333_3333)),
        0x1111_1112
    );
    assert_eq!(unwrap_success(shared.atomic_load_u32(0)), 0x1111_1112);

    assert_eq!(
        unwrap_success(shared.atomic_cmpxchg_u32(0, 0x1111_1112, 0x3333_3333)),
        0x1111_1112
    );
    assert_eq!(unwrap_success(shared.atomic_load_u32(0)), 0x3333_3333);
    assert!(matches!(
        shared.atomic_load_u32(1),
        VMResult::UnalignedAtomic
    ));
}

#[tokio::test]
async fn misaligned_atomic_store_traps_without_partial_write() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (func (export "seed")
            i32.const 0
            i32.const 0x11223344
            i32.store)
          (func (export "misaligned_store")
            i32.const 1
            i32.const 0xaabbccdd
            i32.atomic.store)
          (func (export "load0") (result i32)
            i32.const 0
            i32.load))
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
        run_module_function(
            &instance,
            &store,
            "misaligned_store",
            &ResultValue::new(vec![])
        )
        .await,
        VMResult::UnalignedAtomic
    ));
    assert_eq!(
        unwrap_success(call_i32(&instance, &store, "load0", vec![]).await),
        0x1122_3344
    );
}

#[tokio::test]
async fn indexed_shared_atomic_ops_use_nonzero_memidx() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (memory $m 1 2 shared)
          (func (export "seed") (param i32)
            (i32.atomic.store $m (i32.const 0) (local.get 0)))
          (func (export "load0") (result i32)
            (i32.atomic.load $m (i32.const 0)))
          (func (export "notify0") (result i32)
            (memory.atomic.notify $m (i32.const 0) (i32.const 1)))
          (func (export "wait_not_equal") (result i32)
            (memory.atomic.wait32 $m (i32.const 0) (i32.const 99) (i64.const 0))))
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
            &ResultValue::new(vec![WasmValue::I32(7)]),
        )
        .await,
        VMResult::Success(_)
    ));
    assert_eq!(
        unwrap_success(call_i32(&instance, &store, "load0", vec![]).await),
        7
    );
    assert_eq!(
        unwrap_success(call_i32(&instance, &store, "notify0", vec![]).await),
        0
    );
    assert_eq!(
        unwrap_success(call_i32(&instance, &store, "wait_not_equal", vec![]).await),
        1
    );
}

#[tokio::test]
async fn shared_wait32_timeout_resumes_into_following_ops() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1 1 shared)
          (func (export "seed")
            i32.const 0
            i32.const 7
            i32.atomic.store)
          (func (export "wait_then_add") (result i32)
            i32.const 0
            i32.const 7
            i64.const 0
            memory.atomic.wait32
            i32.const 41
            i32.add))
        "#,
        &store,
        &registry,
    )
    .await;

    assert!(matches!(
        run_module_function(&instance, &store, "seed", &ResultValue::new(vec![])).await,
        VMResult::Success(_)
    ));
    assert_eq!(
        unwrap_success(call_i32(&instance, &store, "wait_then_add", vec![]).await),
        43
    );
}

#[tokio::test]
async fn indexed_shared_wait32_timeout_resumes_into_following_ops() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1)
          (memory $m 1 1 shared)
          (func (export "seed")
            (i32.atomic.store $m (i32.const 0) (i32.const 7)))
          (func (export "wait_then_add") (result i32)
            (memory.atomic.wait32 $m (i32.const 0) (i32.const 7) (i64.const 0))
            i32.const 41
            i32.add))
        "#,
        &store,
        &registry,
    )
    .await;

    assert!(matches!(
        run_module_function(&instance, &store, "seed", &ResultValue::new(vec![])).await,
        VMResult::Success(_)
    ));
    assert_eq!(
        unwrap_success(call_i32(&instance, &store, "wait_then_add", vec![]).await),
        43
    );
}

#[tokio::test]
async fn shared_wait64_timeout_resumes_into_following_ops() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1 1 shared)
          (func (export "seed")
            i32.const 0
            i64.const 7
            i64.atomic.store)
          (func (export "wait_then_add") (result i32)
            i32.const 0
            i64.const 7
            i64.const 0
            memory.atomic.wait64
            i32.const 41
            i32.add))
        "#,
        &store,
        &registry,
    )
    .await;

    assert!(matches!(
        run_module_function(&instance, &store, "seed", &ResultValue::new(vec![])).await,
        VMResult::Success(_)
    ));
    assert_eq!(
        unwrap_success(call_i32(&instance, &store, "wait_then_add", vec![]).await),
        43
    );
}

#[tokio::test]
async fn shared_atomic_wasm_ops_cover_all_widths() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module
          (memory 1 1 shared)

          (func (export "seed8") (param i32 i32)
            local.get 0
            local.get 1
            i32.atomic.store8)
          (func (export "load8") (param i32) (result i32)
            local.get 0
            i32.atomic.load8_u)
          (func (export "rmw8_add") (param i32 i32) (result i32)
            local.get 0
            local.get 1
            i32.atomic.rmw8.add_u)
          (func (export "cmpxchg8") (param i32 i32 i32) (result i32)
            local.get 0
            local.get 1
            local.get 2
            i32.atomic.rmw8.cmpxchg_u)

          (func (export "seed16") (param i32 i32)
            local.get 0
            local.get 1
            i32.atomic.store16)
          (func (export "load16") (param i32) (result i32)
            local.get 0
            i32.atomic.load16_u)
          (func (export "rmw16_xor") (param i32 i32) (result i32)
            local.get 0
            local.get 1
            i32.atomic.rmw16.xor_u)
          (func (export "cmpxchg16") (param i32 i32 i32) (result i32)
            local.get 0
            local.get 1
            local.get 2
            i32.atomic.rmw16.cmpxchg_u)

          (func (export "seed32") (param i32 i32)
            local.get 0
            local.get 1
            i32.atomic.store)
          (func (export "load32") (param i32) (result i32)
            local.get 0
            i32.atomic.load)
          (func (export "rmw32_add") (param i32 i32) (result i32)
            local.get 0
            local.get 1
            i32.atomic.rmw.add)
          (func (export "cmpxchg32") (param i32 i32 i32) (result i32)
            local.get 0
            local.get 1
            local.get 2
            i32.atomic.rmw.cmpxchg)

          (func (export "seed64") (param i32 i64)
            local.get 0
            local.get 1
            i64.atomic.store)
          (func (export "load64") (param i32) (result i64)
            local.get 0
            i64.atomic.load)
          (func (export "rmw64_xchg") (param i32 i64) (result i64)
            local.get 0
            local.get 1
            i64.atomic.rmw.xchg)
          (func (export "cmpxchg64") (param i32 i64 i64) (result i64)
            local.get 0
            local.get 1
            local.get 2
            i64.atomic.rmw.cmpxchg))
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
            &ResultValue::new(vec![WasmValue::I32(0), WasmValue::I32(0x7f)]),
        )
        .await,
        VMResult::Success(_)
    ));
    assert_eq!(
        unwrap_success(
            call_i32(
                &instance,
                &store,
                "rmw8_add",
                vec![WasmValue::I32(0), WasmValue::I32(1)],
            )
            .await,
        ),
        0x7f
    );
    assert_eq!(
        unwrap_success(call_i32(&instance, &store, "load8", vec![WasmValue::I32(0)]).await),
        0x80
    );
    assert_eq!(
        unwrap_success(
            call_i32(
                &instance,
                &store,
                "cmpxchg8",
                vec![
                    WasmValue::I32(0),
                    WasmValue::I32(0x80),
                    WasmValue::I32(0xaa),
                ],
            )
            .await,
        ),
        0x80
    );
    assert_eq!(
        unwrap_success(call_i32(&instance, &store, "load8", vec![WasmValue::I32(0)]).await),
        0xaa
    );

    assert!(matches!(
        run_module_function(
            &instance,
            &store,
            "seed16",
            &ResultValue::new(vec![WasmValue::I32(2), WasmValue::I32(0x1122)]),
        )
        .await,
        VMResult::Success(_)
    ));
    assert_eq!(
        unwrap_success(
            call_i32(
                &instance,
                &store,
                "rmw16_xor",
                vec![WasmValue::I32(2), WasmValue::I32(0x00ff)],
            )
            .await,
        ),
        0x1122
    );
    assert_eq!(
        unwrap_success(call_i32(&instance, &store, "load16", vec![WasmValue::I32(2)]).await),
        0x11dd
    );
    assert_eq!(
        unwrap_success(
            call_i32(
                &instance,
                &store,
                "cmpxchg16",
                vec![
                    WasmValue::I32(2),
                    WasmValue::I32(0x11dd),
                    WasmValue::I32(0xbeef),
                ],
            )
            .await,
        ),
        0x11dd
    );
    assert_eq!(
        unwrap_success(call_i32(&instance, &store, "load16", vec![WasmValue::I32(2)]).await),
        0xbeef
    );

    assert!(matches!(
        run_module_function(
            &instance,
            &store,
            "seed32",
            &ResultValue::new(vec![WasmValue::I32(4), WasmValue::I32(0x3344_5566)]),
        )
        .await,
        VMResult::Success(_)
    ));
    assert_eq!(
        unwrap_success(
            call_i32(
                &instance,
                &store,
                "rmw32_add",
                vec![WasmValue::I32(4), WasmValue::I32(1)],
            )
            .await,
        ),
        0x3344_5566
    );
    assert_eq!(
        unwrap_success(call_i32(&instance, &store, "load32", vec![WasmValue::I32(4)]).await),
        0x3344_5567
    );
    assert_eq!(
        unwrap_success(
            call_i32(
                &instance,
                &store,
                "cmpxchg32",
                vec![
                    WasmValue::I32(4),
                    WasmValue::I32(0x3344_5567),
                    WasmValue::I32(0x4455_6677),
                ],
            )
            .await,
        ),
        0x3344_5567
    );
    assert_eq!(
        unwrap_success(call_i32(&instance, &store, "load32", vec![WasmValue::I32(4)]).await),
        0x4455_6677
    );

    assert!(matches!(
        run_module_function(
            &instance,
            &store,
            "seed64",
            &ResultValue::new(vec![
                WasmValue::I32(8),
                WasmValue::I64(0x0102_0304_0506_0708),
            ]),
        )
        .await,
        VMResult::Success(_)
    ));
    assert_eq!(
        unwrap_success(
            call_i64(
                &instance,
                &store,
                "rmw64_xchg",
                vec![
                    WasmValue::I32(8),
                    WasmValue::I64(0xf0e0_d0c0_b0a0_9080u64 as i64),
                ],
            )
            .await,
        ),
        0x0102_0304_0506_0708
    );
    assert_eq!(
        unwrap_success(call_i64(&instance, &store, "load64", vec![WasmValue::I32(8)]).await),
        0xf0e0_d0c0_b0a0_9080u64 as i64
    );
    assert_eq!(
        unwrap_success(
            call_i64(
                &instance,
                &store,
                "cmpxchg64",
                vec![
                    WasmValue::I32(8),
                    WasmValue::I64(0xf0e0_d0c0_b0a0_9080u64 as i64),
                    WasmValue::I64(0x8877_6655_4433_2211u64 as i64),
                ],
            )
            .await,
        ),
        0xf0e0_d0c0_b0a0_9080u64 as i64
    );
    assert_eq!(
        unwrap_success(call_i64(&instance, &store, "load64", vec![WasmValue::I32(8)]).await),
        0x8877_6655_4433_2211u64 as i64
    );
}
